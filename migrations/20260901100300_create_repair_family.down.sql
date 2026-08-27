-- Reverse the repair family create.
DROP TRIGGER IF EXISTS repair_tags_update_audit ON manufacturing.repair_tags;
DROP TRIGGER IF EXISTS repair_tags_insert_audit ON manufacturing.repair_tags;
DROP FUNCTION IF EXISTS manufacturing.repair_tags_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.repair_tags;

DROP TRIGGER IF EXISTS repair_parts_update_audit ON manufacturing.repair_parts;
DROP TRIGGER IF EXISTS repair_parts_insert_audit ON manufacturing.repair_parts;
DROP FUNCTION IF EXISTS manufacturing.repair_parts_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.repair_parts;

DROP TRIGGER IF EXISTS repair_orders_update_audit ON manufacturing.repair_orders;
DROP TRIGGER IF EXISTS repair_orders_insert_audit ON manufacturing.repair_orders;
DROP FUNCTION IF EXISTS manufacturing.repair_orders_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.repair_orders;

DROP TYPE IF EXISTS repair_line_type;
DROP TYPE IF EXISTS repair_status;
