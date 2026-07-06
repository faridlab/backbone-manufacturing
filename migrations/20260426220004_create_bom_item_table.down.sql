-- Down: drop manufacturing.bom_items table
DROP TABLE IF EXISTS manufacturing.bom_items CASCADE;
DROP FUNCTION IF EXISTS manufacturing.bom_items_audit_timestamp() CASCADE;
