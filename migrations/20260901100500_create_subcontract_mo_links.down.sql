-- Reverse the subcontract link table create.
DROP TRIGGER IF EXISTS subcontract_mo_links_update_audit ON manufacturing.subcontract_mo_links;
DROP TRIGGER IF EXISTS subcontract_mo_links_insert_audit ON manufacturing.subcontract_mo_links;
DROP FUNCTION IF EXISTS manufacturing.subcontract_mo_links_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.subcontract_mo_links;
