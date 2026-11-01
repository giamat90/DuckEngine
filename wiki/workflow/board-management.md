# Work board (Trello) and how agents manage it `[P]`

Decided by the owner (2026-10-24): the board is where the work is managed and where the owner sees what is done and what is in progress. **Agents take care of the cards**: the owner assigns a task or asks "what is the next task?", the agent moves the card to the right column and creates subtasks when needed.

Board: DuckEngine in workspace "Progetti SW" — https://trello.com/b/ZYrtuCUw/duckengine

## 1. Source of truth

Git and the wiki are canonical (roadmap, ADRs, reviews, metric reports). The board is a **view and work queue**. If a card and the repo disagree, the repo wins and the card is fixed. Cards link to the evidence; they do not replace it.

## 2. Columns and who moves a card

| Column | Meaning | Agent moves a card here when |
|---|---|---|
| Backlog | known work, not yet defined as a task | a phase card or an idea is created |
| Ready (task defined) | goal and acceptance criteria written, owner agreed | the owner assigns it, or accepts the agent's proposal for the next task |
| In Progress | TDD work running | the agent starts the first failing test |
| Review (R1-R8) | implementation done, review phase and fix loop | all planned tests are green and the review phase starts |
| Waiting for owner approval | design note, metric report and product view are ready | the review report has 0 open blocker/major issues; the PR is open and its link is given to the owner with the diff hash |
| PR / CI | owner approved the diff hash; checks running or green | the owner approved; the agent merges after green checks |
| Merged in develop | an ordinary card's PR is merged into `develop`, not yet on `main` (cards 1-38, 2-9) | the PR is merged into `develop` |
| Done (released) | the PR is merged into `main` | the PR is merged into `main` (owner decision 2026-10-24); a governance card on its own merge, an ordinary card when the release PR from `develop` merges, and then it gets the version number |

Backward moves are allowed and need a comment saying why (a failed gate, a changed requirement). A card never skips a column.

## 3. Card format

- **Title:** `<phase>-<n> short imperative title`, for example `0a-1 Verify GitHub plan features (V1)`.
- **Id (owner decision 2026-10-26: every ticket always has an id):** a task card gets its id **when it is created**, in any column, including Backlog and ideas. There is no card without an id and no "id to be assigned later". The next free `n` of the phase is found by checking the board (all columns, archived cards included), local and remote `agent/*` branches, the `reviews/` folder and open PRs, and taking the highest used number + 1. The same id names the branch (`agent/<id>-<slug>`), the review report (`reviews/<id>.md`) and appears in the PR title and the commit subject. An id is never reused or renumbered once it appears in a branch, review, commit or PR. The description starts with `Task id: <id>`. Phase cards in Backlog are epics identified by `Phase <p>`; they are not task cards and hold no work of their own.
- **Description:** goal, acceptance criteria (measurable where possible), links to the wiki pages and ADRs, change class once the classifier has run.
- **Checklist:** the test list (TDD step 1) and the review steps R1-R8 as items, ticked as they happen.
- **Comments:** short status notes by the agent: started, blocked, report ready, PR link, release version. Never secrets, tokens or content from outside `DuckEngine/`.
- **Labels:** `blocked`, `class-2`, `class-3`, `spike`, `governance`.

## 4. Subtasks

- Small steps of one task: checklist items on the card.
- Work that needs its own PR (and therefore its own release): a **separate card**, linked from the parent in the description. One card = one PR. An ordinary card is released with its batch (one minor version per release the owner asks for), a governance card gets its own patch version ([delivery-pipeline](delivery-pipeline.md), develop-branch workflow).
- The agent creates subtasks when the test list or the review reveals them, and says so in a comment. It does not silently drop or merge cards.

## 5. "What is the next task?"

When the owner asks, the agent reads the board and the roadmap, proposes one to three candidates from Backlog/Ready with reason and risk, and waits for the owner's choice. It moves nothing to In Progress before the owner picks, and it does not start a phase the owner has not released (phase 0a needs an explicit go-ahead).

## 6. Limits

- Agents work only on the DuckEngine board. They never read or change other boards, workspaces or other Trello data.
- Agents never delete cards or lists; they archive. Card moves and edits are never used to record the owner's approval: approval is given in the session against the diff hash (see [delivery-pipeline](delivery-pipeline.md)).
- Trello is an external service: nothing goes on the board that is not fine to be stored there (no credentials, no private files, no content of the key files in the parent workspace).
- If the Trello connection is unavailable, work continues, but the board is not allowed to stay stale: the agent records the intended card changes in its report as a **to-do for the next moment the connection works (ASAP)**, tells the owner at the next opportunity, and applies the changes first thing when the connection returns, before starting a new task. The task is not reported as fully closed while board updates are pending.
