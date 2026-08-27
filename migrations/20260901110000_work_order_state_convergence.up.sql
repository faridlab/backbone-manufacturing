-- State convergence: the 5-state hand-gated work_order_status (draft/released/in_process/
-- completed/stopped) and 2-state job_card_status (open/completed) converge to the hybrid
-- 6-state work_order_state (draft/confirmed/progress/to_close/done/cancel) and the shop-floor
-- 5-state job_card_state (ready/progress/done/cancel/blocked).
--
-- Postgres cannot DROP enum values, so both enums are REPLACED: new types are created
-- (unqualified, public schema — repo convention from 20260821130000), the columns are cast
-- with a USING CASE mapping, and the old types are dropped.
--
--   work orders : released -> confirmed, in_process -> progress, completed -> done,
--                 draft -> draft (unchanged)
--   job cards   : open -> ready, completed -> done
--
-- `stopped` was unreachable in code and unreachable in every gate (verified at v0.3.2):
-- if any row carries it, the migration RAISES — dirty data refuses LOUDLY, never remapped
-- silently (a silent remap would be a data lie).
--
-- Also adds the reservation_state projection column (availability lives ONLY here — no field
-- named availability exists) and the product_category_id logical FK (whose costing defaults
-- supply accounts when the per-work-order overrides are unset), and swaps the
-- (company_id, status) index onto the new enum.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM manufacturing.work_orders WHERE status = 'stopped') THEN
        RAISE EXCEPTION 'work_orders rows in the stopped state exist; the stopped value was unreachable in code — resolve these rows explicitly before converging';
    END IF;
END
$$;

DO $$
BEGIN
    CREATE TYPE work_order_state AS ENUM ('draft', 'confirmed', 'progress', 'to_close', 'done', 'cancel');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
DO $$
BEGIN
    CREATE TYPE job_card_state AS ENUM ('ready', 'progress', 'done', 'cancel', 'blocked');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
DO $$
BEGIN
    CREATE TYPE reservation_state AS ENUM ('waiting', 'confirmed', 'assigned');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Column defaults must be dropped before a retype (a default cannot auto-cast across
-- the type change) and re-applied after.
ALTER TABLE manufacturing.work_orders ALTER COLUMN status DROP DEFAULT;
ALTER TABLE manufacturing.job_cards ALTER COLUMN status DROP DEFAULT;

ALTER TABLE manufacturing.work_orders
    ALTER COLUMN status TYPE work_order_state USING (
        CASE status::text
            WHEN 'released'   THEN 'confirmed'
            WHEN 'in_process' THEN 'progress'
            WHEN 'completed'  THEN 'done'
            ELSE status::text
        END::work_order_state
    );

ALTER TABLE manufacturing.job_cards
    ALTER COLUMN status TYPE job_card_state USING (
        CASE status::text
            WHEN 'open'      THEN 'ready'
            WHEN 'completed' THEN 'done'
            ELSE status::text
        END::job_card_state
    );

ALTER TABLE manufacturing.work_orders ALTER COLUMN status SET DEFAULT 'draft';
ALTER TABLE manufacturing.job_cards ALTER COLUMN status SET DEFAULT 'ready';

DROP TYPE IF EXISTS work_order_status;
DROP TYPE IF EXISTS job_card_status;

ALTER TABLE manufacturing.work_orders ADD COLUMN reservation_state reservation_state;
ALTER TABLE manufacturing.work_orders ADD COLUMN product_category_id UUID;

DROP INDEX IF EXISTS manufacturing.idx_work_orders_company_id_status;
CREATE INDEX idx_work_orders_company_id_status ON manufacturing.work_orders (company_id, status);
DROP INDEX IF EXISTS manufacturing.idx_job_cards_work_order_id_status;
CREATE INDEX idx_job_cards_work_order_id_status ON manufacturing.job_cards (work_order_id, status);
