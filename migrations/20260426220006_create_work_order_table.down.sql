-- Down: drop manufacturing.work_orders table
DROP TABLE IF EXISTS manufacturing.work_orders CASCADE;
DROP FUNCTION IF EXISTS manufacturing.work_orders_audit_timestamp() CASCADE;
