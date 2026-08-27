-- Down: drop manufacturing.repair_tags table
DROP TABLE IF EXISTS manufacturing.repair_tags CASCADE;
DROP FUNCTION IF EXISTS manufacturing.repair_tags_audit_timestamp() CASCADE;
