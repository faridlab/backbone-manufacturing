-- Reverse the state convergence: restore the 5-state / 2-state enums, dropping the new
-- states' rows loudly where no faithful reverse mapping exists (cancel / to_close / blocked
-- had no counterpart in the old vocabulary; refusing is the honest reverse).
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM manufacturing.work_orders WHERE status IN ('to_close', 'cancel')) THEN
        RAISE EXCEPTION 'work_orders rows in to_close/cancel states exist; the old vocabulary had no counterpart — resolve these rows explicitly before rolling back';
    END IF;
    IF EXISTS (SELECT 1 FROM manufacturing.job_cards WHERE status IN ('cancel', 'blocked', 'ready', 'progress')) THEN
        RAISE EXCEPTION 'job_cards rows in ready/progress/blocked/cancel states exist; the old vocabulary''s only entry states were open/completed — resolve these rows explicitly before rolling back';
    END IF;
END
$$;

ALTER TABLE manufacturing.work_orders DROP COLUMN IF EXISTS reservation_state;
ALTER TABLE manufacturing.work_orders DROP COLUMN IF EXISTS product_category_id;

DO $$
BEGIN
    CREATE TYPE work_order_status AS ENUM ('draft', 'released', 'in_process', 'completed', 'stopped');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
DO $$
BEGIN
    CREATE TYPE job_card_status AS ENUM ('open', 'completed');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Column defaults must be dropped before a retype (a default cannot auto-cast across
-- the type change) and re-applied after.
ALTER TABLE manufacturing.work_orders ALTER COLUMN status DROP DEFAULT;
ALTER TABLE manufacturing.job_cards ALTER COLUMN status DROP DEFAULT;

ALTER TABLE manufacturing.work_orders
    ALTER COLUMN status TYPE work_order_status USING (
        CASE status::text
            WHEN 'confirmed' THEN 'released'
            WHEN 'progress'  THEN 'in_process'
            WHEN 'done'      THEN 'completed'
            ELSE status::text
        END::work_order_status
    );

ALTER TABLE manufacturing.job_cards
    ALTER COLUMN status TYPE job_card_status USING (
        CASE status::text
            WHEN 'done' THEN 'completed'
            ELSE 'open'
        END::job_card_status
    );

ALTER TABLE manufacturing.work_orders ALTER COLUMN status SET DEFAULT 'draft';
ALTER TABLE manufacturing.job_cards ALTER COLUMN status SET DEFAULT 'open';

DROP TYPE IF EXISTS work_order_state;
DROP TYPE IF EXISTS job_card_state;
DROP TYPE IF EXISTS reservation_state;

DROP INDEX IF EXISTS manufacturing.idx_work_orders_company_id_status;
CREATE INDEX idx_work_orders_company_id_status ON manufacturing.work_orders (company_id, status);
DROP INDEX IF EXISTS manufacturing.idx_job_cards_work_order_id_status;
CREATE INDEX idx_job_cards_work_order_id_status ON manufacturing.job_cards (work_order_id, status);
