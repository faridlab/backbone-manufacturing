-- Down: drop manufacturing.category_costing_defaults table
DROP TABLE IF EXISTS manufacturing.category_costing_defaults CASCADE;
DROP FUNCTION IF EXISTS manufacturing.category_costing_defaults_audit_timestamp() CASCADE;
