-- Down: drop manufacturing.subcontract_mo_links table
DROP TABLE IF EXISTS manufacturing.subcontract_mo_links CASCADE;
DROP FUNCTION IF EXISTS manufacturing.subcontract_mo_links_audit_timestamp() CASCADE;
