//! Graph colouring of the contact constraints (card 4-14).
//!
//! Every constraint joins two bodies. Two constraints that share a dynamic body cannot be solved at
//! the same time, because both change that body's velocity. The colouring gives each constraint a
//! colour so that no two constraints of one colour share a dynamic body; a static body, which no
//! constraint changes, is exempt and may appear in any number of constraints of a colour. The solver
//! then goes colour by colour; inside a colour the constraints are independent, so any order, and
//! later any number of threads, give the same bits (card 4-15 relies on exactly this).
//!
//! The colouring is greedy and serial, in the canonical constraint order (the order of the manifold
//! list): a constraint takes the lowest colour that neither of its dynamic bodies uses yet. This is
//! the constraint-graph colouring of `Box2D` v3 **[K]** without its preference for putting the
//! constraints against static bodies into the last colours. It depends on nothing but the input
//! list, so it is the same on every run, thread count and platform.
//!
//! There are at most [`MAX_COLOURS`] colours. A constraint that finds none free goes to the
//! *overflow* set, which is solved after the colours, serially, in canonical order. Running out of
//! colours is a value ([`Colouring::overflow`]), not an error: the step still solves everything.
//!
//! All storage is allocated by [`Colouring::new`].

use super::cells::{change, index_u32, put, read};

/// The most colours; a constraint past them goes to the overflow set. 24, the limit of `Box2D` v3
/// **[K]**: the colour of a body is kept in one `u32` bit mask, and a body is rarely touched by
/// more than a dozen dynamic bodies, so the overflow set is empty in ordinary scenes.
pub const MAX_COLOURS: usize = 24;

/// The mask with one bit per colour.
const ALL_COLOURS: u32 = (1 << MAX_COLOURS) - 1;

/// The colour value of a constraint that is not coloured (it has no dynamic body to protect).
const UNCOLOURED: u8 = u8::MAX;

/// The colour value of a constraint in the overflow set.
const OVERFLOW: u8 = 24;

/// The bodies of one constraint, by slot index. `None` is a body the colouring does not protect: a
/// static body, or a body that does not move this step. A constraint with two `None` ends is not
/// solved and is not coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstraintEnds {
    /// Slot index of the first body, if it moves.
    pub a: Option<u32>,
    /// Slot index of the second body, if it moves.
    pub b: Option<u32>,
}

impl ConstraintEnds {
    /// The slot indices of the ends that move, `a` first.
    fn moving_ends(self) -> impl Iterator<Item = u32> {
        [self.a, self.b].into_iter().flatten()
    }
}

/// The colour of one constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    /// A colour from 0 to [`MAX_COLOURS`] - 1.
    Index(u8),
    /// The serial overflow set, solved after all colours.
    Overflow,
    /// Not coloured: the constraint touches no moving body.
    None,
}

/// The colouring of one step's constraints. Reused every step.
pub struct Colouring {
    ends: Vec<ConstraintEnds>,
    capacity: usize,
    /// The colour of each constraint (a colour index, [`OVERFLOW`] or [`UNCOLOURED`]).
    colour: Vec<u8>,
    /// For each body slot, the colours its constraints already use (one bit per colour).
    used: Vec<u32>,
    /// The coloured constraints, colour by colour, overflow last; canonical order inside a colour.
    order: Vec<u32>,
    /// `starts[c]..starts[c + 1]` is the part of `order` that holds colour `c`; index
    /// [`MAX_COLOURS`] is the overflow set.
    starts: [u32; MAX_COLOURS + 2],
    colours: usize,
}

impl Colouring {
    /// A colouring for up to `max_constraints` constraints between `max_bodies` bodies.
    #[must_use]
    pub fn new(max_bodies: usize, max_constraints: usize) -> Self {
        Self {
            ends: Vec::with_capacity(max_constraints),
            capacity: max_constraints,
            colour: vec![UNCOLOURED; max_constraints],
            used: vec![0; max_bodies],
            order: Vec::with_capacity(max_constraints),
            starts: [0; MAX_COLOURS + 2],
            colours: 0,
        }
    }

    /// Forgets the constraints of the last step.
    pub fn clear(&mut self) {
        self.ends.clear();
        self.order.clear();
        self.starts = [0; MAX_COLOURS + 2];
        self.colours = 0;
    }

    /// Adds the next constraint in canonical order. A constraint past the capacity is ignored; the
    /// capacity is the number of manifolds the solver can hold, so the solver never sends more.
    pub fn push(&mut self, ends: ConstraintEnds) {
        if self.ends.len() < self.capacity {
            self.ends.push(ends);
        }
    }

    /// The number of constraints pushed since [`Colouring::clear`].
    #[must_use]
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// True when no constraint was pushed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Colours the pushed constraints and builds the solve order.
    pub fn assign(&mut self) {
        self.reset_masks();
        for (index, ends) in self.ends.iter().enumerate() {
            put(&mut self.colour, index, pick(&mut self.used, *ends));
        }
        self.arrange();
    }

    /// Clears the colour mask of every body a pushed constraint names (only those: the cost is the
    /// constraint count, not the body count).
    fn reset_masks(&mut self) {
        for end in self.ends.iter().flat_map(|ends| ends.moving_ends()) {
            put(&mut self.used, end as usize, 0);
        }
    }

    /// Counting sort of the constraint indices by colour; stable, so the canonical order is kept
    /// inside a colour.
    fn arrange(&mut self) {
        let counts = self.count_colours();
        let total = self.lay_out(counts);
        self.colours = counts
            .iter()
            .take(MAX_COLOURS)
            .rposition(|&count| count > 0)
            .map_or(0, |last| last + 1);
        self.fill_order(total);
    }

    /// How many constraints each colour holds (the last entry is the overflow set).
    fn count_colours(&self) -> [u32; MAX_COLOURS + 1] {
        let mut counts = [0_u32; MAX_COLOURS + 1];
        for &colour in self.colour.iter().take(self.ends.len()) {
            change(&mut counts, usize::from(colour), |count| *count += 1);
        }
        counts
    }

    /// Sets `starts` from the per-colour `counts`; returns the number of coloured constraints.
    fn lay_out(&mut self, counts: [u32; MAX_COLOURS + 1]) -> u32 {
        let mut total = 0_u32;
        for (start, count) in self.starts.iter_mut().zip(counts) {
            *start = total;
            total += count;
        }
        put(&mut self.starts, MAX_COLOURS + 1, total);
        total
    }

    /// Writes the constraint indices into `order`, each at the next free place of its colour.
    fn fill_order(&mut self, total: u32) {
        self.order.clear();
        self.order.resize(total as usize, 0);
        let mut next = self.starts;
        for (index, &colour) in self.colour.iter().take(self.ends.len()).enumerate() {
            let place = read(&next, usize::from(colour), u32::MAX);
            put(&mut self.order, place as usize, index_u32(index));
            change(&mut next, usize::from(colour), |cursor| *cursor += 1);
        }
    }

    /// The colour of constraint `index` (its position in the order pushed).
    #[must_use]
    pub fn colour_of(&self, index: usize) -> Colour {
        match self.colour.get(index).filter(|_| index < self.ends.len()) {
            Some(&UNCOLOURED) | None => Colour::None,
            Some(&OVERFLOW) => Colour::Overflow,
            Some(&colour) => Colour::Index(colour),
        }
    }

    /// The number of colours in use: one more than the highest colour given; the overflow set is
    /// not counted. The colours in use are always 0 to this number minus one.
    #[must_use]
    pub fn colour_count(&self) -> usize {
        self.colours
    }

    /// The constraints of `colour`, in canonical order; empty for a colour not in use.
    #[must_use]
    pub fn members(&self, colour: usize) -> &[u32] {
        if colour >= MAX_COLOURS {
            return &[];
        }
        self.span(colour)
    }

    /// The constraints that did not fit into [`MAX_COLOURS`] colours, in canonical order.
    #[must_use]
    pub fn overflow(&self) -> &[u32] {
        self.span(MAX_COLOURS)
    }

    /// Every coloured constraint in solve order: colour 0, colour 1, ..., then the overflow set.
    #[must_use]
    pub fn order(&self) -> &[u32] {
        &self.order
    }

    /// The part of the order that holds `slot` (a colour index or [`MAX_COLOURS`]).
    fn span(&self, slot: usize) -> &[u32] {
        let range = self.starts.get(slot).zip(self.starts.get(slot + 1));
        range
            .and_then(|(&from, &to)| self.order.get(from as usize..to as usize))
            .unwrap_or(&[])
    }

    /// The part of the order that holds `colour`, for the test hooks.
    #[cfg(test)]
    fn part_mut(&mut self, colour: usize) -> Option<&mut [u32]> {
        let from = read(&self.starts, colour, 0) as usize;
        let to = read(&self.starts, colour + 1, 0) as usize;
        self.order.get_mut(from..to)
    }

    /// Reorders the constraints inside each colour (and inside the overflow set, which stays
    /// serial and so is left alone) by a Fisher-Yates shuffle from `seed`. For tests of the
    /// property that the order inside a colour does not matter.
    #[cfg(test)]
    pub(crate) fn shuffle_within_colours(&mut self, seed: u64) {
        let mut rng = base::rng::Pcg32::new(seed, 7);
        for colour in 0..MAX_COLOURS {
            self.part_mut(colour)
                .into_iter()
                .for_each(|part| shuffle(part, &mut rng));
        }
    }

    /// Reverses the order inside each colour. For tests.
    #[cfg(test)]
    pub(crate) fn reverse_within_colours(&mut self) {
        for colour in 0..MAX_COLOURS {
            self.part_mut(colour).into_iter().for_each(<[u32]>::reverse);
        }
    }
}

/// The lowest colour neither moving end of `ends` uses, marked as used by both; the overflow value
/// when there is none; [`UNCOLOURED`] when no end moves.
fn pick(used: &mut [u32], ends: ConstraintEnds) -> u8 {
    if ends.a.is_none() && ends.b.is_none() {
        return UNCOLOURED;
    }
    let taken = ends
        .moving_ends()
        .fold(0, |mask, end| mask | read(used, end as usize, 0));
    let free = !taken & ALL_COLOURS;
    if free == 0 {
        return OVERFLOW;
    }
    let colour = free.trailing_zeros();
    for end in ends.moving_ends() {
        change(used, end as usize, |mask| *mask |= 1 << colour);
    }
    // `colour` is below MAX_COLOURS (24), so it fits a byte.
    u8::try_from(colour).unwrap_or(OVERFLOW)
}

/// Fisher-Yates shuffle of `part`. For the test hooks.
#[cfg(test)]
fn shuffle(part: &mut [u32], rng: &mut base::rng::Pcg32) {
    for i in (1..part.len()).rev() {
        let bound = u32::try_from(i + 1).unwrap_or(1);
        let j = rng.below(bound).unwrap_or(0) as usize;
        part.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::rng::Pcg32;

    fn moving(a: u32, b: u32) -> ConstraintEnds {
        ConstraintEnds {
            a: Some(a),
            b: Some(b),
        }
    }

    fn against_static(a: u32) -> ConstraintEnds {
        ConstraintEnds {
            a: Some(a),
            b: None,
        }
    }

    fn coloured(bodies: usize, list: &[ConstraintEnds]) -> Colouring {
        let mut colouring = Colouring::new(bodies, list.len());
        for &ends in list {
            colouring.push(ends);
        }
        colouring.assign();
        colouring
    }

    /// The slot indices of the moving ends of the members of `colour`.
    fn bodies_of(colouring: &Colouring, list: &[ConstraintEnds], colour: usize) -> Vec<u32> {
        colouring
            .members(colour)
            .iter()
            .filter_map(|&index| list.get(index as usize))
            .flat_map(|ends| ends.moving_ends())
            .collect()
    }

    /// The properties of a colouring of `list`: valid, complete, canonical inside a colour.
    fn check(colouring: &Colouring, list: &[ConstraintEnds]) {
        for (index, ends) in list.iter().enumerate() {
            let expected = usize::from(ends.a.is_some() || ends.b.is_some());
            let listed = colouring
                .order()
                .iter()
                .filter(|&&listed| listed as usize == index)
                .count();
            assert_eq!(listed, expected, "constraint {index}");
        }
        for colour in 0..MAX_COLOURS {
            assert!(
                colouring.members(colour).windows(2).all(|w| w[0] < w[1]),
                "canonical order"
            );
            let bodies = bodies_of(colouring, list, colour);
            let repeated = (0..bodies.len()).any(|i| bodies[..i].contains(&bodies[i]));
            assert!(!repeated, "colour {colour} shares a body");
        }
        assert!(colouring.overflow().windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn the_overflow_value_is_the_colour_limit() {
        assert_eq!(usize::from(OVERFLOW), MAX_COLOURS);
    }

    #[test]
    fn nothing_pushed_gives_an_empty_colouring() {
        let colouring = coloured(4, &[]);
        assert!(colouring.is_empty());
        assert_eq!(colouring.len(), 0);
        assert_eq!(colouring.colour_count(), 0);
        assert!(colouring.order().is_empty() && colouring.overflow().is_empty());
        assert_eq!(colouring.colour_of(0), Colour::None);
    }

    #[test]
    fn constraints_that_share_no_body_share_colour_zero() {
        let list = [moving(0, 1), moving(2, 3), moving(4, 5)];
        let colouring = coloured(6, &list);
        assert_eq!(colouring.colour_count(), 1);
        assert_eq!(colouring.members(0), &[0, 1, 2]);
        assert_eq!(colouring.colour_of(1), Colour::Index(0));
        check(&colouring, &list);
    }

    #[test]
    fn a_chain_alternates_two_colours() {
        let list = [moving(0, 1), moving(1, 2), moving(2, 3), moving(3, 4)];
        let colouring = coloured(5, &list);
        assert_eq!(colouring.colour_count(), 2);
        assert_eq!(colouring.members(0), &[0, 2]);
        assert_eq!(colouring.members(1), &[1, 3]);
        assert_eq!(colouring.order(), &[0, 2, 1, 3]);
        check(&colouring, &list);
    }

    #[test]
    fn a_triangle_needs_three_colours() {
        let list = [moving(0, 1), moving(1, 2), moving(0, 2)];
        let colouring = coloured(3, &list);
        assert_eq!(colouring.colour_count(), 3);
        assert_eq!(
            (0..3).map(|i| colouring.colour_of(i)).collect::<Vec<_>>(),
            [Colour::Index(0), Colour::Index(1), Colour::Index(2)]
        );
        check(&colouring, &list);
    }

    #[test]
    fn a_static_body_is_exempt_so_all_its_constraints_share_a_colour() {
        let list = [against_static(0), against_static(1), against_static(2)];
        let colouring = coloured(3, &list);
        assert_eq!(colouring.colour_count(), 1);
        assert_eq!(colouring.members(0), &[0, 1, 2]);
        check(&colouring, &list);
    }

    #[test]
    fn a_body_used_twice_against_static_needs_two_colours() {
        let list = [against_static(0), against_static(0)];
        let colouring = coloured(1, &list);
        assert_eq!(colouring.colour_count(), 2);
        check(&colouring, &list);
    }

    #[test]
    fn a_constraint_with_no_moving_end_is_not_coloured() {
        let none = ConstraintEnds { a: None, b: None };
        let list = [none, moving(0, 1), none];
        let colouring = coloured(2, &list);
        assert_eq!(colouring.colour_of(0), Colour::None);
        assert_eq!(colouring.colour_of(1), Colour::Index(0));
        assert_eq!(colouring.colour_of(2), Colour::None);
        assert_eq!(colouring.order(), &[1]);
        assert_eq!(colouring.colour_of(3), Colour::None);
        check(&colouring, &list);
    }

    #[test]
    fn the_colour_of_a_free_colour_is_the_lowest_one() {
        // 0-1 gets colour 0, 1-2 colour 1, 3-4 colour 0 again, 2-3 must avoid colours 1 and 0.
        let list = [moving(0, 1), moving(1, 2), moving(3, 4), moving(2, 3)];
        let colouring = coloured(5, &list);
        assert_eq!(colouring.colour_of(2), Colour::Index(0));
        assert_eq!(colouring.colour_of(3), Colour::Index(2));
        check(&colouring, &list);
    }

    #[test]
    fn a_star_past_the_colour_limit_overflows_in_canonical_order() {
        let count = MAX_COLOURS + 3;
        let leaves = u32::try_from(count).unwrap_or(0);
        let list: Vec<_> = (1..=leaves).map(|leaf| moving(0, leaf)).collect();
        let colouring = coloured(count + 1, &list);
        assert_eq!(colouring.colour_count(), MAX_COLOURS);
        assert_eq!(colouring.overflow(), &[24, 25, 26]);
        assert_eq!(colouring.colour_of(MAX_COLOURS), Colour::Overflow);
        assert_eq!(colouring.colour_of(MAX_COLOURS - 1), Colour::Index(23));
        assert_eq!(colouring.order().len(), count);
        assert_eq!(colouring.order().last(), Some(&26));
        check(&colouring, &list);
    }

    #[test]
    fn exactly_the_colour_limit_does_not_overflow() {
        let leaves = u32::try_from(MAX_COLOURS).unwrap_or(0);
        let list: Vec<_> = (1..=leaves).map(|leaf| moving(0, leaf)).collect();
        let colouring = coloured(MAX_COLOURS + 1, &list);
        assert_eq!(colouring.colour_count(), MAX_COLOURS);
        assert!(colouring.overflow().is_empty());
        check(&colouring, &list);
    }

    #[test]
    fn pushing_past_the_capacity_is_ignored() {
        let mut colouring = Colouring::new(4, 1);
        colouring.push(moving(0, 1));
        colouring.push(moving(2, 3));
        colouring.assign();
        assert_eq!(colouring.len(), 1);
        assert!(!colouring.is_empty());
        assert_eq!(colouring.order(), &[0]);
    }

    #[test]
    fn clear_forgets_everything_and_the_colouring_is_reusable() {
        let mut colouring = coloured(3, &[moving(0, 1), moving(1, 2)]);
        colouring.clear();
        assert!(colouring.is_empty());
        assert_eq!(colouring.colour_count(), 0);
        assert!(colouring.order().is_empty());
        colouring.push(moving(0, 1));
        colouring.assign();
        assert_eq!(colouring.members(0), &[0]);
        // Constraint 1 of the earlier round left its colour behind; it is not a constraint now.
        assert_eq!(colouring.colour_of(1), Colour::None);
        assert_eq!(colouring.members(1), &[] as &[u32]);
        assert_eq!(colouring.members(MAX_COLOURS), &[] as &[u32]);
    }

    #[test]
    fn a_body_name_outside_the_body_range_is_tolerated() {
        let colouring = coloured(2, &[moving(0, 99)]);
        assert_eq!(colouring.order(), &[0]);
    }

    fn random_ends(rng: &mut Pcg32, bodies: u32) -> ConstraintEnds {
        let a = rng.below(bodies).unwrap_or(0);
        let b = rng.below(bodies).unwrap_or(0);
        ConstraintEnds {
            a: (rng.below(5).unwrap_or(0) != 0).then_some(a),
            b: (rng.below(5).unwrap_or(0) != 0).then_some(b),
        }
    }

    /// A seeded random constraint list over a seeded random number of bodies.
    fn random_graph(seed: u64) -> (u32, Vec<ConstraintEnds>) {
        let mut rng = Pcg32::new(seed, 1);
        let bodies = 3 + rng.below(40).unwrap_or(0);
        let count = rng.below(120).unwrap_or(0) as usize;
        let list = (0..count)
            .map(|_| random_ends(&mut rng, bodies))
            .filter(|ends| ends.a != ends.b || ends.a.is_none())
            .collect();
        (bodies, list)
    }

    #[test]
    fn random_graphs_are_valid_complete_and_deterministic() {
        for seed in 0..40_u64 {
            let (bodies, list) = random_graph(seed);
            let first = coloured(bodies as usize, &list);
            let second = coloured(bodies as usize, &list);
            check(&first, &list);
            assert_eq!(first.order(), second.order(), "seed {seed}");
        }
    }

    /// The two lists hold the same constraints (each constraint is in a colour once).
    fn same_members(a: &[u32], b: &[u32]) -> bool {
        a.len() == b.len() && a.iter().all(|member| b.contains(member))
    }

    #[test]
    fn shuffling_and_reversing_keep_each_colour_a_permutation_of_itself() {
        let list: Vec<_> = (0..30_u32).map(|i| moving(i, i + 1)).collect();
        let reference = coloured(31, &list);
        let mut shuffled = coloured(31, &list);
        shuffled.shuffle_within_colours(3);
        let mut reversed = coloured(31, &list);
        reversed.reverse_within_colours();
        assert_ne!(shuffled.order(), reference.order());
        assert_ne!(reversed.order(), reference.order());
        for colour in 0..reference.colour_count() {
            assert!(same_members(
                shuffled.members(colour),
                reference.members(colour)
            ));
            assert!(same_members(
                reversed.members(colour),
                reference.members(colour)
            ));
        }
    }
}
