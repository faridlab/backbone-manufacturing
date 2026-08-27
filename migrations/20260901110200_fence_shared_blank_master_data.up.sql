-- shared_blank company fence for master data (ADR-0014) + the BoM/workstation columns the
-- convergence schema adds to existing tables.
--
-- Posture map (schema/models/index.model.yaml):
--   shared_blank (nullable company_id — NULL rows are shared, visible to every company
--   session, the port of Odoo's [False] company on master data):
--     workstations, operations, boms, bom_items, bom_operations
--   strict (NOT NULL company_id, unchanged here): work_orders, work_order_items, job_cards
--   and every other transaction table.
--
-- The DROP NOT NULL and the policy recreation with the `OR company_id IS NULL` arm land in
-- the SAME migration: a session that applied one without the other would either see shared
-- rows it must not see, or no longer be able to insert shared rows at all.
--
-- Unique posture in a shared namespace: with a nullable company_id, plain partial uniques
-- treat NULL as distinct, so two shared rows with the same code would NOT collide. The
-- affected uniques are recreated NULLS NOT DISTINCT (PG 15+) so the shared namespace stays
-- collision-free too.

ALTER TABLE manufacturing.workstations  ALTER COLUMN company_id DROP NOT NULL;
ALTER TABLE manufacturing.operations    ALTER COLUMN company_id DROP NOT NULL;
ALTER TABLE manufacturing.boms          ALTER COLUMN company_id DROP NOT NULL;
ALTER TABLE manufacturing.bom_items     ALTER COLUMN company_id DROP NOT NULL;
ALTER TABLE manufacturing.bom_operations ALTER COLUMN company_id DROP NOT NULL;

DROP POLICY IF EXISTS workstations_company_isolation ON manufacturing.workstations;
CREATE POLICY workstations_company_isolation ON manufacturing.workstations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS operations_company_isolation ON manufacturing.operations;
CREATE POLICY operations_company_isolation ON manufacturing.operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS boms_company_isolation ON manufacturing.boms;
CREATE POLICY boms_company_isolation ON manufacturing.boms
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS bom_items_company_isolation ON manufacturing.bom_items;
CREATE POLICY bom_items_company_isolation ON manufacturing.bom_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS bom_operations_company_isolation ON manufacturing.bom_operations;
CREATE POLICY bom_operations_company_isolation ON manufacturing.bom_operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

-- Shared-namespace uniques: recreate NULLS NOT DISTINCT so the (NULL, code) slot is single.
DROP INDEX IF EXISTS manufacturing.idx_boms_company_id_bom_code;
CREATE UNIQUE INDEX idx_boms_company_id_bom_code ON manufacturing.boms (company_id, bom_code) NULLS NOT DISTINCT WHERE (metadata->>'deleted_at') IS NULL;

-- BoM staging: version + type. Draft becomes a third bom_status value (Postgres cannot
-- drop enum values; the type is only ever grown here). One live BoM per
-- (company, item, version); a NULL company takes the shared slot.
DO $$ BEGIN
    CREATE TYPE bom_type AS ENUM ('normal', 'kit', 'subcontract');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    ALTER TYPE bom_status ADD VALUE IF NOT EXISTS 'draft';
END $$;

ALTER TABLE manufacturing.boms ADD COLUMN version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0);
ALTER TABLE manufacturing.boms ADD COLUMN bom_type bom_type NOT NULL DEFAULT 'normal';

DROP INDEX IF EXISTS manufacturing.idx_boms_company_id_item_id_status;
CREATE INDEX idx_boms_company_id_item_id_status ON manufacturing.boms (company_id, item_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_boms_company_item_version ON manufacturing.boms (company_id, item_id, version) NULLS NOT DISTINCT WHERE (metadata->>'deleted_at') IS NULL;

-- Workstation capacity/efficiency (the OEE inputs): capacity must be able to produce
-- something (strictly positive), efficiency can be zero (a down station) but not negative.
ALTER TABLE manufacturing.workstations ADD COLUMN capacity NUMERIC(18,4) NOT NULL DEFAULT 1 CHECK (capacity > 0);
ALTER TABLE manufacturing.workstations ADD COLUMN time_efficiency NUMERIC(5,2) NOT NULL DEFAULT 100 CHECK (time_efficiency >= 0);
