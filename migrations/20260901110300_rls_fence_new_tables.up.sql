-- Company RLS fence for every NEW table of the convergence (ADR-0008, ADR-0014).
-- Hand-authored: the generated enable_company_rls migration predates these tables and
-- a regen does not rewrite it, so this carries the fence for them explicitly.
--
-- Posture map (schema/models/index.model.yaml):
--   shared_blank (nullable company_id, NULL rows are shared master data):
--     workstation_losses, bom_byproducts, bom_subcontractors
--   strict (NOT NULL company_id — the IS NULL arm is dead by the column constraint):
--     workstation_productivity, unbuild_orders, repair_orders, repair_parts,
--     repair_tags, category_costing_defaults, subcontract_mo_links
--
-- company_id is scoped per request via `set_config('app.company_id', <uuid>, true)`;
-- an unset var sees zero rows (plus shared rows on the shared_blank tables).
-- Requires the app to connect as a non-superuser role; migrations/seeders run as the
-- owner and bypass.

-- shared_blank (master data) ------------------------------------------------------

ALTER TABLE manufacturing.workstation_losses ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.workstation_losses FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workstation_losses_company_isolation ON manufacturing.workstation_losses;
CREATE POLICY workstation_losses_company_isolation ON manufacturing.workstation_losses
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

ALTER TABLE manufacturing.bom_byproducts ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.bom_byproducts FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS bom_byproducts_company_isolation ON manufacturing.bom_byproducts;
CREATE POLICY bom_byproducts_company_isolation ON manufacturing.bom_byproducts
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

ALTER TABLE manufacturing.bom_subcontractors ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.bom_subcontractors FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS bom_subcontractors_company_isolation ON manufacturing.bom_subcontractors;
CREATE POLICY bom_subcontractors_company_isolation ON manufacturing.bom_subcontractors
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

-- strict (transactions) -----------------------------------------------------------

ALTER TABLE manufacturing.workstation_productivity ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.workstation_productivity FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS workstation_productivity_company_isolation ON manufacturing.workstation_productivity;
CREATE POLICY workstation_productivity_company_isolation ON manufacturing.workstation_productivity
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE manufacturing.unbuild_orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.unbuild_orders FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS unbuild_orders_company_isolation ON manufacturing.unbuild_orders;
CREATE POLICY unbuild_orders_company_isolation ON manufacturing.unbuild_orders
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE manufacturing.repair_orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.repair_orders FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS repair_orders_company_isolation ON manufacturing.repair_orders;
CREATE POLICY repair_orders_company_isolation ON manufacturing.repair_orders
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE manufacturing.repair_parts ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.repair_parts FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS repair_parts_company_isolation ON manufacturing.repair_parts;
CREATE POLICY repair_parts_company_isolation ON manufacturing.repair_parts
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE manufacturing.repair_tags ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.repair_tags FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS repair_tags_company_isolation ON manufacturing.repair_tags;
CREATE POLICY repair_tags_company_isolation ON manufacturing.repair_tags
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE manufacturing.category_costing_defaults ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.category_costing_defaults FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS category_costing_defaults_company_isolation ON manufacturing.category_costing_defaults;
CREATE POLICY category_costing_defaults_company_isolation ON manufacturing.category_costing_defaults
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE manufacturing.subcontract_mo_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.subcontract_mo_links FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS subcontract_mo_links_company_isolation ON manufacturing.subcontract_mo_links;
CREATE POLICY subcontract_mo_links_company_isolation ON manufacturing.subcontract_mo_links
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
