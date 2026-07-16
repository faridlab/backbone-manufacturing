-- Down: remove the company RLS fence for manufacturing module

-- Reverse the company RLS fence for manufacturing.workstations
DROP POLICY IF EXISTS workstations_company_isolation ON manufacturing.workstations;
ALTER TABLE manufacturing.workstations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.workstations DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for manufacturing.operations
DROP POLICY IF EXISTS operations_company_isolation ON manufacturing.operations;
ALTER TABLE manufacturing.operations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.operations DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for manufacturing.boms
DROP POLICY IF EXISTS boms_company_isolation ON manufacturing.boms;
ALTER TABLE manufacturing.boms NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.boms DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for manufacturing.work_orders
DROP POLICY IF EXISTS work_orders_company_isolation ON manufacturing.work_orders;
ALTER TABLE manufacturing.work_orders NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.work_orders DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for manufacturing.job_cards
DROP POLICY IF EXISTS job_cards_company_isolation ON manufacturing.job_cards;
ALTER TABLE manufacturing.job_cards NO FORCE ROW LEVEL SECURITY;
ALTER TABLE manufacturing.job_cards DISABLE ROW LEVEL SECURITY;

