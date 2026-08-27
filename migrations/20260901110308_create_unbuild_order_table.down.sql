-- Down: drop manufacturing.unbuild_orders table
DROP TABLE IF EXISTS manufacturing.unbuild_orders CASCADE;
DROP FUNCTION IF EXISTS manufacturing.unbuild_orders_audit_timestamp() CASCADE;
