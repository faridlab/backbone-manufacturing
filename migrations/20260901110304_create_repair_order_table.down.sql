-- Down: drop manufacturing.repair_orders table
DROP TABLE IF EXISTS manufacturing.repair_orders CASCADE;
DROP FUNCTION IF EXISTS manufacturing.repair_orders_audit_timestamp() CASCADE;
