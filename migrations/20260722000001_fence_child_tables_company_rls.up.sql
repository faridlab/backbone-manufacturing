-- Company row-level-security fence for the three manufacturing CHILD tables (ADR-0010 Decision A).
--
-- bom_items / bom_operations / work_order_items were the only tables in the module still
-- un-fenced: the parent headers (boms, work_orders) carry company_id and are already fenced by
-- 20260426220009_enable_company_rls, but their lines had no company_id column — a cross-tenant
-- read of a line was possible if the caller knew a line id. This adds a direct company_id to each
-- child (denormalized from the parent — a logical FK, NO hard SQL FK to organization.companies)
-- and enables + forces the same USING/WITH CHECK policy the parents use.
--
-- Backfill is deterministic: every parent row already carries a non-null company_id, so the join
-- cannot leave a NULL behind — no fail-loud guard needed. .down.sql reverses verbatim.

-- =============================================================================
-- manufacturing.bom_items
-- =============================================================================
ALTER TABLE manufacturing.bom_items ADD COLUMN company_id UUID;

UPDATE manufacturing.bom_items AS c
   SET company_id = p.company_id
  FROM manufacturing.boms AS p
 WHERE c.bom_id = p.id;

ALTER TABLE manufacturing.bom_items ALTER COLUMN company_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS idx_bom_items_company_id ON manufacturing.bom_items (company_id);

ALTER TABLE manufacturing.bom_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.bom_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS bom_items_company_isolation ON manufacturing.bom_items;
CREATE POLICY bom_items_company_isolation ON manufacturing.bom_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- =============================================================================
-- manufacturing.bom_operations
-- =============================================================================
ALTER TABLE manufacturing.bom_operations ADD COLUMN company_id UUID;

UPDATE manufacturing.bom_operations AS c
   SET company_id = p.company_id
  FROM manufacturing.boms AS p
 WHERE c.bom_id = p.id;

ALTER TABLE manufacturing.bom_operations ALTER COLUMN company_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS idx_bom_operations_company_id ON manufacturing.bom_operations (company_id);

ALTER TABLE manufacturing.bom_operations ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.bom_operations FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS bom_operations_company_isolation ON manufacturing.bom_operations;
CREATE POLICY bom_operations_company_isolation ON manufacturing.bom_operations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- =============================================================================
-- manufacturing.work_order_items
-- =============================================================================
ALTER TABLE manufacturing.work_order_items ADD COLUMN company_id UUID;

UPDATE manufacturing.work_order_items AS c
   SET company_id = p.company_id
  FROM manufacturing.work_orders AS p
 WHERE c.work_order_id = p.id;

ALTER TABLE manufacturing.work_order_items ALTER COLUMN company_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS idx_work_order_items_company_id ON manufacturing.work_order_items (company_id);

ALTER TABLE manufacturing.work_order_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.work_order_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS work_order_items_company_isolation ON manufacturing.work_order_items;
CREATE POLICY work_order_items_company_isolation ON manufacturing.work_order_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
