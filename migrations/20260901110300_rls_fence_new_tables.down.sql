-- Reverse the RLS fence for the new tables.
DROP POLICY IF EXISTS subcontract_mo_links_company_isolation ON manufacturing.subcontract_mo_links;
ALTER TABLE manufacturing.subcontract_mo_links NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS category_costing_defaults_company_isolation ON manufacturing.category_costing_defaults;
ALTER TABLE manufacturing.category_costing_defaults NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS repair_tags_company_isolation ON manufacturing.repair_tags;
ALTER TABLE manufacturing.repair_tags NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS repair_parts_company_isolation ON manufacturing.repair_parts;
ALTER TABLE manufacturing.repair_parts NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS repair_orders_company_isolation ON manufacturing.repair_orders;
ALTER TABLE manufacturing.repair_orders NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS unbuild_orders_company_isolation ON manufacturing.unbuild_orders;
ALTER TABLE manufacturing.unbuild_orders NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS workstation_productivity_company_isolation ON manufacturing.workstation_productivity;
ALTER TABLE manufacturing.workstation_productivity NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS bom_subcontractors_company_isolation ON manufacturing.bom_subcontractors;
ALTER TABLE manufacturing.bom_subcontractors NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS bom_byproducts_company_isolation ON manufacturing.bom_byproducts;
ALTER TABLE manufacturing.bom_byproducts NO FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS workstation_losses_company_isolation ON manufacturing.workstation_losses;
ALTER TABLE manufacturing.workstation_losses NO FORCE ROW LEVEL SECURITY;
