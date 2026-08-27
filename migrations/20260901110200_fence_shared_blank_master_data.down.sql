-- Reverse the shared_blank fence. Refuses loudly if shared (NULL-company) rows exist —
-- deleting master data to satisfy a rollback would be a silent loss.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM manufacturing.workstations  WHERE company_id IS NULL)
    OR EXISTS (SELECT 1 FROM manufacturing.operations    WHERE company_id IS NULL)
    OR EXISTS (SELECT 1 FROM manufacturing.boms          WHERE company_id IS NULL)
    OR EXISTS (SELECT 1 FROM manufacturing.bom_items     WHERE company_id IS NULL)
    OR EXISTS (SELECT 1 FROM manufacturing.bom_operations WHERE company_id IS NULL) THEN
        RAISE EXCEPTION 'shared (NULL company_id) master-data rows exist; assign them to a company before rolling back the shared_blank fence';
    END IF;
END
$$;

ALTER TABLE manufacturing.workstations  ALTER COLUMN company_id SET NOT NULL;
ALTER TABLE manufacturing.operations    ALTER COLUMN company_id SET NOT NULL;
ALTER TABLE manufacturing.boms          ALTER COLUMN company_id SET NOT NULL;
ALTER TABLE manufacturing.bom_items     ALTER COLUMN company_id SET NOT NULL;
ALTER TABLE manufacturing.bom_operations ALTER COLUMN company_id SET NOT NULL;

DROP POLICY IF EXISTS workstations_company_isolation ON manufacturing.workstations;
CREATE POLICY workstations_company_isolation ON manufacturing.workstations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS operations_company_isolation ON manufacturing.operations;
CREATE POLICY operations_company_isolation ON manufacturing.operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS boms_company_isolation ON manufacturing.boms;
CREATE POLICY boms_company_isolation ON manufacturing.boms
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS bom_items_company_isolation ON manufacturing.bom_items;
CREATE POLICY bom_items_company_isolation ON manufacturing.bom_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS bom_operations_company_isolation ON manufacturing.bom_operations;
CREATE POLICY bom_operations_company_isolation ON manufacturing.bom_operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP INDEX IF EXISTS manufacturing.idx_boms_company_id_bom_code;
CREATE UNIQUE INDEX idx_boms_company_id_bom_code ON manufacturing.boms (company_id, bom_code) WHERE (metadata->>'deleted_at') IS NULL;

DROP INDEX IF EXISTS manufacturing.idx_boms_company_item_version;
DROP INDEX IF EXISTS manufacturing.idx_boms_company_id_item_id_status;
CREATE INDEX idx_boms_company_id_item_id_status ON manufacturing.boms (company_id, item_id, status);

ALTER TABLE manufacturing.boms DROP COLUMN IF EXISTS bom_type;
ALTER TABLE manufacturing.boms DROP COLUMN IF EXISTS version;
DROP TYPE IF EXISTS bom_type;

-- Remove the draft enum value by replacing the type (Postgres cannot drop values).
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM manufacturing.boms WHERE status = 'draft') THEN
        RAISE EXCEPTION 'boms rows still in draft; close or delete them before rolling back';
    END IF;
END
$$;
DO $$ BEGIN
    CREATE TYPE bom_status_rollback AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
ALTER TABLE manufacturing.boms ALTER COLUMN status DROP DEFAULT;
ALTER TABLE manufacturing.boms
    ALTER COLUMN status TYPE bom_status_rollback USING (CASE status::text WHEN 'draft' THEN 'active' ELSE status::text END::bom_status_rollback);
ALTER TABLE manufacturing.boms ALTER COLUMN status SET DEFAULT 'active';
DROP TYPE bom_status;
ALTER TYPE bom_status_rollback RENAME TO bom_status;

ALTER TABLE manufacturing.workstations DROP COLUMN IF EXISTS time_efficiency;
ALTER TABLE manufacturing.workstations DROP COLUMN IF EXISTS capacity;
