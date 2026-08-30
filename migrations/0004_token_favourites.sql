-- Lets a tracked token be starred, so the tokens worth watching daily are reachable
-- without paging past the ones that are merely tracked.
--
-- A favourite is a property of a *tracked* token rather than a separate list of mints.
-- A token only enters `tokens` after the price provider has priced it, so anything
-- starrable is already something an alert can fire on; a parallel list of starred mints
-- could hold entries that no rule could ever use.
--
-- Favourites are shared, not per-user, matching every other table here: rules, targets
-- and history are already global to the deployment and every admin sees the same ones.
-- A per-user column would be the only per-user state in the schema and would imply an
-- isolation the rest of the product does not provide.
--
-- Stored as a nullable timestamp rather than a boolean, mirroring `users.blocked_at`:
-- it answers "since when", which is what gives favourites a stable order.
ALTER TABLE tokens ADD COLUMN favourited_at TEXT;

-- The favourites screen reads exactly this predicate.
CREATE INDEX idx_tokens_favourited ON tokens(favourited_at) WHERE favourited_at IS NOT NULL;
