-- Reverse the unbuild orders create.
DROP TRIGGER IF EXISTS unbuild_orders_update_audit ON manufacturing.unbuild_orders;
DROP TRIGGER IF EXISTS unbuild_orders_insert_audit ON manufacturing.unbuild_orders;
DROP FUNCTION IF EXISTS manufacturing.unbuild_orders_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.unbuild_orders;
DROP TYPE IF EXISTS unbuild_status;
