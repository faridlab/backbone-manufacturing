-- Migration: replace the three manufacturing lifecycle booleans with status enums
-- workstations, operations and boms each carried `is_active BOOLEAN NOT NULL DEFAULT TRUE`;
-- the tree-wide convention is one `status` enum field per lifecycle (see
-- docs/refactoring-schema in the serpa workspace). Each boolean migrates only rows deviating
-- from its own column default; the dependent (company_id, is_active) indexes are dropped with
-- the column and replaced by status-shaped ones. The enum types are created unqualified so they
-- land beside the module's other enum types (public), where the generated sqlx type_name resolves.

DO $$ BEGIN
    CREATE TYPE workstation_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
DO $$ BEGIN
    CREATE TYPE operation_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
DO $$ BEGIN
    CREATE TYPE bom_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

ALTER TABLE manufacturing.workstations ADD COLUMN status workstation_status NOT NULL DEFAULT 'active';
UPDATE manufacturing.workstations SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE manufacturing.workstations DROP COLUMN is_active;
CREATE INDEX IF NOT EXISTS idx_workstations_company_id_status ON manufacturing.workstations (company_id, status);

ALTER TABLE manufacturing.operations ADD COLUMN status operation_status NOT NULL DEFAULT 'active';
UPDATE manufacturing.operations SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE manufacturing.operations DROP COLUMN is_active;
CREATE INDEX IF NOT EXISTS idx_operations_company_id_status ON manufacturing.operations (company_id, status);

ALTER TABLE manufacturing.boms ADD COLUMN status bom_status NOT NULL DEFAULT 'active';
UPDATE manufacturing.boms SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE manufacturing.boms DROP COLUMN is_active;
CREATE INDEX IF NOT EXISTS idx_boms_company_id_item_id_status ON manufacturing.boms (company_id, item_id, status);
