-- Down: restore the three is_active booleans exactly as they were.
-- Only 'inactive' rows are written back as FALSE; rows at the column default
-- map to the boolean default TRUE without an UPDATE. The status-shaped indexes
-- are dropped with the status column; the original (company_id, is_active)
-- indexes are recreated by their original names.

ALTER TABLE manufacturing.workstations ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE manufacturing.workstations SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE manufacturing.workstations DROP COLUMN status;
CREATE INDEX IF NOT EXISTS idx_workstations_company_id_is_active ON manufacturing.workstations (company_id, is_active);

ALTER TABLE manufacturing.operations ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE manufacturing.operations SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE manufacturing.operations DROP COLUMN status;
CREATE INDEX IF NOT EXISTS idx_operations_company_id_is_active ON manufacturing.operations (company_id, is_active);

ALTER TABLE manufacturing.boms ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE manufacturing.boms SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE manufacturing.boms DROP COLUMN status;
CREATE INDEX IF NOT EXISTS idx_boms_company_id_item_id_is_active ON manufacturing.boms (company_id, item_id, is_active);

DROP TYPE IF EXISTS workstation_status;
DROP TYPE IF EXISTS operation_status;
DROP TYPE IF EXISTS bom_status;
