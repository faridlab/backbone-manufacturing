-- Down: drop manufacturing.work_order_items table
DROP TABLE IF EXISTS manufacturing.work_order_items CASCADE;
DROP FUNCTION IF EXISTS manufacturing.work_order_items_audit_timestamp() CASCADE;
