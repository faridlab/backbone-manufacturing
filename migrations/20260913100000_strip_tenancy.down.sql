-- Hand-authored (user-owned). Not regenerated.
--
-- Reverse the tenancy strip: restore the module-native company fence the module
-- declared before ADR-0029 (strict NOT NULL company_id on the transaction family;
-- nullable shared_blank company_id on the master-data family). Rows the decorator
-- moved to org_unit_id keep their org anchor — this down file only re-adds the
-- columns, the indexes and the policies; it does not move data back.

-- TRANSACTIONS (strict NOT NULL company_id).
ALTER TABLE manufacturing.category_costing_defaults ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.job_cards                ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.repair_orders            ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.repair_parts             ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.repair_tags              ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.subcontract_mo_links     ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.unbuild_orders           ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.work_order_items         ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.work_orders              ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE manufacturing.workstation_productivity ADD COLUMN IF NOT EXISTS company_id UUID NOT NULL DEFAULT gen_random_uuid();

-- MASTER DATA (shared_blank — nullable company_id, NULL = the one shared set).
ALTER TABLE manufacturing.bom_byproducts     ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.bom_items          ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.bom_operations     ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.bom_subcontractors ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.boms               ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.operations         ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.workstation_losses ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE manufacturing.workstations       ADD COLUMN IF NOT EXISTS company_id UUID;

CREATE INDEX IF NOT EXISTS idx_bom_items_company_id
    ON manufacturing.bom_items (company_id);
CREATE INDEX IF NOT EXISTS idx_bom_operations_company_id
    ON manufacturing.bom_operations (company_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_boms_company_id_bom_code
    ON manufacturing.boms (company_id, bom_code) NULLS NOT DISTINCT
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE INDEX IF NOT EXISTS idx_boms_company_id_item_id_status
    ON manufacturing.boms (company_id, item_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_boms_company_item_version
    ON manufacturing.boms (company_id, item_id, version) NULLS NOT DISTINCT
    WHERE ((metadata->>'deleted_at') IS NULL);
-- The historical chain emitted this pair twice under two names (same columns,
-- same predicate); both are restored so the down shape matches the pre-strip DDL.
CREATE UNIQUE INDEX IF NOT EXISTS idx_category_costing_defaults_company_category
    ON manufacturing.category_costing_defaults (company_id, product_category_id)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE UNIQUE INDEX IF NOT EXISTS idx_category_costing_defaults_company_id_product_category_id
    ON manufacturing.category_costing_defaults (company_id, product_category_id)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE INDEX IF NOT EXISTS idx_operations_company_id_status
    ON manufacturing.operations (company_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repair_orders_company_id_repair_number
    ON manufacturing.repair_orders (company_id, repair_number)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE INDEX IF NOT EXISTS idx_repair_orders_company_id_status
    ON manufacturing.repair_orders (company_id, status);
CREATE INDEX IF NOT EXISTS idx_repair_parts_company_id
    ON manufacturing.repair_parts (company_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repair_tags_company_id_name
    ON manufacturing.repair_tags (company_id, name)
    WHERE ((metadata->>'deleted_at') IS NULL);
-- Same historical duplicate-pair situation as category_costing_defaults above.
CREATE UNIQUE INDEX IF NOT EXISTS idx_subcontract_mo_links_company_id_purchase_order_id
    ON manufacturing.subcontract_mo_links (company_id, purchase_order_id)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE UNIQUE INDEX IF NOT EXISTS idx_subcontract_mo_links_company_po
    ON manufacturing.subcontract_mo_links (company_id, purchase_order_id)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE INDEX IF NOT EXISTS idx_unbuild_orders_company_id_status
    ON manufacturing.unbuild_orders (company_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_unbuild_orders_company_id_unbuild_number
    ON manufacturing.unbuild_orders (company_id, unbuild_number)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE INDEX IF NOT EXISTS idx_work_order_items_company_id
    ON manufacturing.work_order_items (company_id);
CREATE INDEX IF NOT EXISTS idx_work_orders_company_id_status
    ON manufacturing.work_orders (company_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_work_orders_company_id_work_order_number
    ON manufacturing.work_orders (company_id, work_order_number)
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE UNIQUE INDEX IF NOT EXISTS idx_workstation_losses_company_id_name
    ON manufacturing.workstation_losses (company_id, name) NULLS NOT DISTINCT
    WHERE ((metadata->>'deleted_at') IS NULL);
CREATE INDEX IF NOT EXISTS idx_workstation_productivity_company_id
    ON manufacturing.workstation_productivity (company_id);
CREATE INDEX IF NOT EXISTS idx_workstations_company_id_status
    ON manufacturing.workstations (company_id, status);

-- TRANSACTIONS: strict isolation (equality only — the IS NULL arm is dead here).
DROP POLICY IF EXISTS category_costing_defaults_company_isolation ON manufacturing.category_costing_defaults;
CREATE POLICY category_costing_defaults_company_isolation ON manufacturing.category_costing_defaults
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS job_cards_company_isolation ON manufacturing.job_cards;
CREATE POLICY job_cards_company_isolation ON manufacturing.job_cards
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS repair_orders_company_isolation ON manufacturing.repair_orders;
CREATE POLICY repair_orders_company_isolation ON manufacturing.repair_orders
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS repair_parts_company_isolation ON manufacturing.repair_parts;
CREATE POLICY repair_parts_company_isolation ON manufacturing.repair_parts
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS repair_tags_company_isolation ON manufacturing.repair_tags;
CREATE POLICY repair_tags_company_isolation ON manufacturing.repair_tags
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS subcontract_mo_links_company_isolation ON manufacturing.subcontract_mo_links;
CREATE POLICY subcontract_mo_links_company_isolation ON manufacturing.subcontract_mo_links
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS unbuild_orders_company_isolation ON manufacturing.unbuild_orders;
CREATE POLICY unbuild_orders_company_isolation ON manufacturing.unbuild_orders
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS work_order_items_company_isolation ON manufacturing.work_order_items;
CREATE POLICY work_order_items_company_isolation ON manufacturing.work_order_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS work_orders_company_isolation ON manufacturing.work_orders;
CREATE POLICY work_orders_company_isolation ON manufacturing.work_orders
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

DROP POLICY IF EXISTS workstation_productivity_company_isolation ON manufacturing.workstation_productivity;
CREATE POLICY workstation_productivity_company_isolation ON manufacturing.workstation_productivity
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- MASTER DATA: the shared_blank shape (NULL = the one shared set).
DROP POLICY IF EXISTS bom_byproducts_company_isolation ON manufacturing.bom_byproducts;
CREATE POLICY bom_byproducts_company_isolation ON manufacturing.bom_byproducts
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS bom_items_company_isolation ON manufacturing.bom_items;
CREATE POLICY bom_items_company_isolation ON manufacturing.bom_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS bom_operations_company_isolation ON manufacturing.bom_operations;
CREATE POLICY bom_operations_company_isolation ON manufacturing.bom_operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS bom_subcontractors_company_isolation ON manufacturing.bom_subcontractors;
CREATE POLICY bom_subcontractors_company_isolation ON manufacturing.bom_subcontractors
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS boms_company_isolation ON manufacturing.boms;
CREATE POLICY boms_company_isolation ON manufacturing.boms
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS operations_company_isolation ON manufacturing.operations;
CREATE POLICY operations_company_isolation ON manufacturing.operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS workstation_losses_company_isolation ON manufacturing.workstation_losses;
CREATE POLICY workstation_losses_company_isolation ON manufacturing.workstation_losses
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);

DROP POLICY IF EXISTS workstations_company_isolation ON manufacturing.workstations;
CREATE POLICY workstations_company_isolation ON manufacturing.workstations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid OR company_id IS NULL);
