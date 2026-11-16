//! Typed queries with declared access and parallel iteration (design note section 3, ADR-0003).
//!
//! A query names the components it reads and writes as types: [`Read<T>`] and [`Write<T>`], alone
//! or in a tuple of two or three. The access it declares is data ([`Access`]), checked before
//! anything runs: a query that names a component twice conflicts with itself and is refused. Two
//! queries may run at the same time only if [`Access::conflicts_with`] says no; running different
//! systems in parallel is a later card, the check is here from the start.
//!
//! Iteration is table by table in creation order, row by row, so the order is a function of the
//! history of the world and never of a hash. [`World::par_for_each`] splits each table into at
//! most [`MAX_CHUNKS`] contiguous chunks and runs them on an [`IScheduler`]; the closure sees one
//! entity at a time and may touch only what the item gives it, so the result does not depend on
//! the grouping, the order or the thread (D0, D1). All of it is safe code, without allocation.

use crate::archetype::Archetype;
use crate::column::{Column, typed, typed_mut};
use crate::component::{Component, ComponentId};
use crate::error::EcsError;
use crate::world::World;
use jobs::scheduler::IScheduler;
use std::any::TypeId;
use std::iter::Zip;
use std::marker::PhantomData;
use std::sync::{Mutex, PoisonError};

/// Most components one query can name.
pub const MAX_PARAMS: usize = 3;

/// Most chunks a table is split into by [`World::par_for_each`] (the chunks live on the stack).
pub const MAX_CHUNKS: usize = 64;

mod sealed {
    pub trait Sealed {}
}

/// Reads component `T`: the closure receives `&T`.
pub struct Read<T>(PhantomData<fn() -> T>);

/// Writes component `T`: the closure receives `&mut T`.
pub struct Write<T>(PhantomData<fn() -> T>);

/// One component a query names, and whether it writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    type_id: TypeId,
    writes: bool,
}

impl Probe {
    fn of<T: Component>(writes: bool) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            writes,
        }
    }
}

/// A run of consecutive rows of one or several columns that can be cut in two.
pub trait Rows: IntoIterator + Sized {
    /// Number of rows.
    fn rows(&self) -> usize;
    /// Cuts into the first `at` rows and the rest (`at` past the end gives everything and an
    /// empty rest).
    fn split(self, at: usize) -> (Self, Self);
}

impl<T> Rows for &[T] {
    fn rows(&self) -> usize {
        self.len()
    }

    fn split(self, at: usize) -> (Self, Self) {
        self.split_at(at.min(self.len()))
    }
}

impl<T> Rows for &mut [T] {
    fn rows(&self) -> usize {
        self.len()
    }

    fn split(self, at: usize) -> (Self, Self) {
        let at = at.min(self.len());
        self.split_at_mut(at)
    }
}

/// Two runs walked together.
pub struct Rows2<A, B>(A, B);

impl<A: Rows, B: Rows> IntoIterator for Rows2<A, B> {
    type Item = (A::Item, B::Item);
    type IntoIter = Zip<A::IntoIter, B::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().zip(self.1)
    }
}

impl<A: Rows, B: Rows> Rows for Rows2<A, B> {
    fn rows(&self) -> usize {
        self.0.rows().min(self.1.rows())
    }

    fn split(self, at: usize) -> (Self, Self) {
        let (a1, a2) = self.0.split(at);
        let (b1, b2) = self.1.split(at);
        (Self(a1, b1), Self(a2, b2))
    }
}

/// Three runs walked together.
pub struct Rows3<A, B, C>(A, B, C);

/// The iterator of [`Rows3`].
pub struct Zip3<A, B, C> {
    a: A,
    b: B,
    c: C,
}

impl<A: Iterator, B: Iterator, C: Iterator> Iterator for Zip3<A, B, C> {
    type Item = (A::Item, B::Item, C::Item);

    fn next(&mut self) -> Option<Self::Item> {
        Some((self.a.next()?, self.b.next()?, self.c.next()?))
    }
}

impl<A: Rows, B: Rows, C: Rows> IntoIterator for Rows3<A, B, C> {
    type Item = (A::Item, B::Item, C::Item);
    type IntoIter = Zip3<A::IntoIter, B::IntoIter, C::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
        Zip3 {
            a: self.0.into_iter(),
            b: self.1.into_iter(),
            c: self.2.into_iter(),
        }
    }
}

impl<A: Rows, B: Rows, C: Rows> Rows for Rows3<A, B, C> {
    fn rows(&self) -> usize {
        self.0.rows().min(self.1.rows()).min(self.2.rows())
    }

    fn split(self, at: usize) -> (Self, Self) {
        let (a1, a2) = self.0.split(at);
        let (b1, b2) = self.1.split(at);
        let (c1, c2) = self.2.split(at);
        (Self(a1, b1, c1), Self(a2, b2, c2))
    }
}

/// The columns a table hands to a query, in the order the query names its components.
pub type Columns<'a> = [Option<&'a mut dyn Column>; MAX_PARAMS];

/// One component of a query: [`Read<T>`] or [`Write<T>`]. Sealed.
pub trait Param: sealed::Sealed {
    /// What the closure receives for this component.
    type Item<'a>;
    /// The rows of the column.
    type Slice<'a>: Rows<Item = Self::Item<'a>> + Send;
    /// The component and whether it is written.
    fn probe() -> Probe;
    /// The rows of `column`, when it holds this component.
    fn slice(column: &mut dyn Column) -> Option<Self::Slice<'_>>;
}

impl<T> sealed::Sealed for Read<T> {}
impl<T> sealed::Sealed for Write<T> {}

impl<T: Component> Param for Read<T> {
    type Item<'a> = &'a T;
    type Slice<'a> = &'a [T];

    fn probe() -> Probe {
        Probe::of::<T>(false)
    }

    fn slice(column: &mut dyn Column) -> Option<&[T]> {
        typed::<T>(column).map(Vec::as_slice)
    }
}

impl<T: Component> Param for Write<T> {
    type Item<'a> = &'a mut T;
    type Slice<'a> = &'a mut [T];

    fn probe() -> Probe {
        Probe::of::<T>(true)
    }

    fn slice(column: &mut dyn Column) -> Option<&mut [T]> {
        typed_mut::<T>(column).map(Vec::as_mut_slice)
    }
}

/// What a query names: one [`Param`], or a tuple of two or three. Sealed.
pub trait Query: sealed::Sealed {
    /// What the closure receives per entity.
    type Item<'a>;
    /// The rows of the columns, walked together.
    type Slices<'a>: Rows<Item = Self::Item<'a>> + Send;
    /// The components named, in order; unused places are `None`.
    fn probes() -> [Option<Probe>; MAX_PARAMS];
    /// The rows of the columns a table handed over, when it has them all.
    fn slices(columns: Columns<'_>) -> Option<Self::Slices<'_>>;
}

impl<P: Param> Query for P {
    type Item<'a> = P::Item<'a>;
    type Slices<'a> = P::Slice<'a>;

    fn probes() -> [Option<Probe>; MAX_PARAMS] {
        [Some(P::probe()), None, None]
    }

    fn slices(columns: Columns<'_>) -> Option<Self::Slices<'_>> {
        let [first, _, _] = columns;
        P::slice(first?)
    }
}

impl<A: Param, B: Param> sealed::Sealed for (A, B) {}
impl<A: Param, B: Param, C: Param> sealed::Sealed for (A, B, C) {}

impl<A: Param, B: Param> Query for (A, B) {
    type Item<'a> = (A::Item<'a>, B::Item<'a>);
    type Slices<'a> = Rows2<A::Slice<'a>, B::Slice<'a>>;

    fn probes() -> [Option<Probe>; MAX_PARAMS] {
        [Some(A::probe()), Some(B::probe()), None]
    }

    fn slices(columns: Columns<'_>) -> Option<Self::Slices<'_>> {
        let [first, second, _] = columns;
        Some(Rows2(A::slice(first?)?, B::slice(second?)?))
    }
}

impl<A: Param, B: Param, C: Param> Query for (A, B, C) {
    type Item<'a> = (A::Item<'a>, B::Item<'a>, C::Item<'a>);
    type Slices<'a> = Rows3<A::Slice<'a>, B::Slice<'a>, C::Slice<'a>>;

    fn probes() -> [Option<Probe>; MAX_PARAMS] {
        [Some(A::probe()), Some(B::probe()), Some(C::probe())]
    }

    fn slices(columns: Columns<'_>) -> Option<Self::Slices<'_>> {
        let [first, second, third] = columns;
        Some(Rows3(
            A::slice(first?)?,
            B::slice(second?)?,
            C::slice(third?)?,
        ))
    }
}

/// The components a query reads and writes, as data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Access {
    reads: Vec<TypeId>,
    writes: Vec<TypeId>,
}

impl Access {
    /// The access query `Q` declares (this allocates; it is for scheduling and tests, not for the
    /// iteration path).
    ///
    /// # Errors
    /// [`EcsError::ConflictingAccess`] when `Q` names a component twice.
    pub fn of<Q: Query>() -> Result<Self, EcsError> {
        let probes = Q::probes();
        check_distinct(&probes)?;
        let ids = |writes: bool| -> Vec<TypeId> {
            probes
                .iter()
                .flatten()
                .filter(|probe| probe.writes == writes)
                .map(|probe| probe.type_id)
                .collect()
        };
        let access = Self {
            reads: ids(false),
            writes: ids(true),
        };
        Ok(access)
    }

    /// The components only read.
    #[must_use]
    pub fn reads(&self) -> &[TypeId] {
        &self.reads
    }

    /// The components written.
    #[must_use]
    pub fn writes(&self) -> &[TypeId] {
        &self.writes
    }

    /// Whether running both at the same time could race: one writes a component the other reads
    /// or writes.
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        let touches =
            |access: &Self, id: &TypeId| access.reads.contains(id) || access.writes.contains(id);
        self.writes.iter().any(|id| touches(other, id))
            || other.writes.iter().any(|id| touches(self, id))
    }
}

/// A component named twice, whatever the access, would need two borrows of one column.
fn check_distinct(probes: &[Option<Probe>; MAX_PARAMS]) -> Result<(), EcsError> {
    for (at, probe) in probes.iter().enumerate() {
        let repeated = probe.is_some_and(|p| {
            probes
                .iter()
                .skip(at + 1)
                .flatten()
                .any(|later| later.type_id == p.type_id)
        });
        if repeated {
            return Err(EcsError::ConflictingAccess);
        }
    }
    Ok(())
}

/// The components of a query as ids of this world; `None` when one was never registered, so no
/// entity can match.
struct Plan {
    ids: [ComponentId; MAX_PARAMS],
    count: usize,
}

impl Plan {
    fn ids(&self) -> &[ComponentId] {
        self.ids.get(..self.count).unwrap_or_default()
    }
}

/// The ids of the components of `probes` in `world`; `None` when one was never registered.
fn plan_of(world: &World, probes: &[Option<Probe>; MAX_PARAMS]) -> Option<Plan> {
    let mut plan = Plan {
        ids: [ComponentId(0); MAX_PARAMS],
        count: 0,
    };
    for (slot, probe) in plan.ids.iter_mut().zip(probes.iter().flatten()) {
        *slot = world.component_id(probe.type_id)?;
        plan.count += 1;
    }
    Some(plan)
}

impl World {
    fn plan<Q: Query>(&self) -> Result<Option<Plan>, EcsError> {
        let probes = Q::probes();
        check_distinct(&probes)?;
        Ok(plan_of(self, &probes))
    }

    /// Runs `f` on every entity that has all the components of `Q`, tables in creation order and
    /// rows in order. Does nothing when no entity matches.
    ///
    /// # Errors
    /// [`EcsError::ConflictingAccess`] when `Q` names a component twice; nothing ran.
    pub fn for_each<Q: Query, F>(&mut self, mut f: F) -> Result<(), EcsError>
    where
        F: for<'a> FnMut(Q::Item<'a>),
    {
        let Some(plan) = self.plan::<Q>()? else {
            return Ok(());
        };
        visit::<Q, F>(self.tables_mut(), &plan, &mut f);
        Ok(())
    }

    /// Like [`World::for_each`], with each table cut into chunks of at least `grain` rows, the last of a table excepted (at most
    /// [`MAX_CHUNKS`] chunks) that run on `scheduler`. The closure must treat each entity on its
    /// own: its effect then equals the serial loop whatever the order, the grouping and the
    /// thread. A table with a single chunk runs on the calling thread without a scheduler call.
    ///
    /// # Errors
    /// [`EcsError::ConflictingAccess`]; [`EcsError::Jobs`] when `grain` is zero or the scheduler
    /// refuses a call (the tables before it were already done).
    pub fn par_for_each<Q: Query, F>(
        &mut self,
        scheduler: &dyn IScheduler,
        grain: u32,
        f: F,
    ) -> Result<(), EcsError>
    where
        F: for<'a> Fn(Q::Item<'a>) + Sync,
    {
        if grain == 0 {
            return Err(EcsError::Jobs(jobs::error::JobError::InvalidGrain));
        }
        let Some(plan) = self.plan::<Q>()? else {
            return Ok(());
        };
        visit_par::<Q, F>(self.tables_mut(), &plan, scheduler, grain, &f)
    }
}

fn visit<Q: Query, F>(tables: &mut [Archetype], plan: &Plan, f: &mut F)
where
    F: for<'a> FnMut(Q::Item<'a>),
{
    for table in tables {
        if let Some(slices) = Q::slices(table.take_columns(plan.ids())) {
            slices.into_iter().for_each(&mut *f);
        }
    }
}

fn visit_par<Q: Query, F>(
    tables: &mut [Archetype],
    plan: &Plan,
    scheduler: &dyn IScheduler,
    grain: u32,
    f: &F,
) -> Result<(), EcsError>
where
    F: for<'a> Fn(Q::Item<'a>) + Sync,
{
    for table in tables {
        if let Some(slices) = Q::slices(table.take_columns(plan.ids())) {
            run_chunks(slices, scheduler, grain, f)?;
        }
    }
    Ok(())
}

/// Rows per chunk: at least `grain`, and few enough that [`MAX_CHUNKS`] chunks cover `rows`.
fn rows_per_chunk(rows: usize, grain: u32) -> usize {
    let grain = usize::try_from(grain).unwrap_or(usize::MAX);
    grain.max(rows.div_ceil(MAX_CHUNKS)).max(1)
}

fn run_chunks<S, F>(
    slices: S,
    scheduler: &dyn IScheduler,
    grain: u32,
    f: &F,
) -> Result<(), EcsError>
where
    S: Rows + Send,
    F: Fn(S::Item) + Sync,
{
    let per_chunk = rows_per_chunk(slices.rows(), grain);
    let chunks = slices.rows().div_ceil(per_chunk);
    if chunks <= 1 {
        slices.into_iter().for_each(f);
        return Ok(());
    }
    let cells: [Mutex<Option<S>>; MAX_CHUNKS] = std::array::from_fn(|_| Mutex::new(None));
    deal(slices, per_chunk, chunks, &cells);
    let count = u32::try_from(chunks).map_err(|_| EcsError::TooManyEntities)?;
    scheduler
        .parallel_for(count, 1, &|range| {
            for at in range {
                run_cell(&cells, at, f);
            }
        })
        .map_err(EcsError::Jobs)
}

/// Cuts `slices` into `chunks` pieces of `per_chunk` rows (the last may be shorter) and puts
/// each into its cell.
fn deal<S: Rows>(slices: S, per_chunk: usize, chunks: usize, cells: &[Mutex<Option<S>>]) {
    let mut rest = Some(slices);
    for cell in cells.iter().take(chunks) {
        let Some(whole) = rest.take() else {
            break;
        };
        let (head, tail) = whole.split(per_chunk);
        *cell.lock().unwrap_or_else(PoisonError::into_inner) = Some(head);
        rest = Some(tail);
    }
}

/// Runs the chunk in cell `at`, once: a second visit finds the cell empty.
fn run_cell<S, F>(cells: &[Mutex<Option<S>>], at: u32, f: &F)
where
    S: Rows,
    F: Fn(S::Item),
{
    let taken = usize::try_from(at)
        .ok()
        .and_then(|at| cells.get(at))
        .and_then(|cell| cell.lock().unwrap_or_else(PoisonError::into_inner).take());
    if let Some(chunk) = taken {
        chunk.into_iter().for_each(f);
    }
}
