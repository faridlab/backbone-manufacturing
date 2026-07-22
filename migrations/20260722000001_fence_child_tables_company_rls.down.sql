-- Down: reverse the company RLS fence on the three manufacturing child tables (ADR-0010 Decision A).
-- Reverses 20260722000001_fence_child_tables_company_rls.up.sql verbatim.

-- =============================================================================
-- manufacturing.work_order_items
-- =============================================================================
DROP POLICY IF EXISTS work_order_items_company_isolation ON manufacturing.work_order_items;
ALTER TABLE manufacturing.work_order_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.work_order_items DISABLE ROW LEVEL SECURITY;
DROP INDEX IF EXISTS manufacturing.idx_work_order_items_company_id;
ALTER TABLE manufacturing.work_order_items DROP COLUMN IF EXISTS company_id;

-- =============================================================================
-- manufacturing.bom_operations
-- =============================================================================
DROP POLICY IF EXISTS bom_operations_company_isolation ON manufacturing.bom_operations;
ALTER TABLE manufacturing.bom_operations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.bom_operations DISABLE ROW LEVEL SECURITY;
DROP INDEX IF EXISTS manufacturing.idx_bom_operations_company_id;
ALTER TABLE manufacturing.bom_operations DROP COLUMN IF EXISTS company_id;

-- =============================================================================
-- manufacturing.bom_items
-- =============================================================================
DROP POLICY IF EXISTS bom_items_company_isolation ON manufacturing.bom_items;
ALTER TABLE manufacturing.bom_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.bom_items DISABLE ROW LEVEL SECURITY;
DROP INDEX IF EXISTS manufacturing.idx_bom_items_company_id;
ALTER TABLE manufacturing.bom_items DROP COLUMN IF EXISTS company_id;
