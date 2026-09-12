-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the manufacturing tables (ADR-0029): the
-- module is tenant-agnostic; org scoping is installed by the COMPOSING service's
-- tenancy decorator, never by the module. Dropped here, per table: the
-- <table>_company_isolation RLS policy, the company-leading indexes and uniques,
-- and the company_id column.
--
-- The company-leading UNIQUES the module used to declare (all of them partial on
-- deleted_at IS NULL) become the decorator's org_unit_id-leading re-declarations:
-- boms (org, bom_code), boms (org, item, version), workstation_losses (org, name)
-- — NULLS NOT DISTINCT so the root-shared set and unit-owned rows keep one
-- namespace — category_costing_defaults (org, category), repair_orders (org,
-- repair_number), repair_tags (org, name), subcontract_mo_links (org, purchase
-- order), unbuild_orders (org, unbuild_number), work_orders (org, work order
-- number).
--
-- The composed-world shapes: the master-data family (workstations,
-- workstation_losses, operations, boms + bom_items/bom_operations/bom_byproducts/
-- bom_subcontractors — the old shared_blank set) fences with allow_root — rows the
-- old policies' NULL arm admitted become tenant-root-anchored at backfill, and the
-- scope union (subtree ∪ tenant root) carries the one-shared-set semantics; a
-- master anchored at a regular unit stays that unit's private master. The
-- transaction family (work_orders, work_order_items, job_cards,
-- workstation_productivity, unbuild_orders, repair_orders, repair_parts,
-- repair_tags, category_costing_defaults, subcontract_mo_links) is plain
-- org-scoped rows (decorator fill stamps the acting unit).
--
-- Ordering guard (the decorator must run FIRST on any database with data): the
-- module never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from
--      company_id — or b) it is empty (a fresh database).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping
-- a column that still holds the only tenancy key. The file is re-runnable
-- (every drop is IF EXISTS and the tracker has no checksums), so a failed run
-- retries cleanly after the decorator lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those
-- now.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY['bom_byproducts', 'bom_items', 'bom_operations',
                             'bom_subcontractors', 'boms', 'category_costing_defaults',
                             'job_cards', 'operations', 'repair_orders', 'repair_parts',
                             'repair_tags', 'subcontract_mo_links', 'unbuild_orders',
                             'work_order_items', 'work_orders', 'workstation_losses',
                             'workstation_productivity', 'workstations']
    LOOP
        IF to_regclass(format('manufacturing.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'manufacturing' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM manufacturing.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM manufacturing.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' manufacturing.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

DROP POLICY IF EXISTS bom_byproducts_company_isolation           ON manufacturing.bom_byproducts;
DROP POLICY IF EXISTS bom_items_company_isolation                ON manufacturing.bom_items;
DROP POLICY IF EXISTS bom_operations_company_isolation           ON manufacturing.bom_operations;
DROP POLICY IF EXISTS bom_subcontractors_company_isolation       ON manufacturing.bom_subcontractors;
DROP POLICY IF EXISTS boms_company_isolation                     ON manufacturing.boms;
DROP POLICY IF EXISTS category_costing_defaults_company_isolation ON manufacturing.category_costing_defaults;
DROP POLICY IF EXISTS job_cards_company_isolation                ON manufacturing.job_cards;
DROP POLICY IF EXISTS operations_company_isolation               ON manufacturing.operations;
DROP POLICY IF EXISTS repair_orders_company_isolation            ON manufacturing.repair_orders;
DROP POLICY IF EXISTS repair_parts_company_isolation             ON manufacturing.repair_parts;
DROP POLICY IF EXISTS repair_tags_company_isolation              ON manufacturing.repair_tags;
DROP POLICY IF EXISTS subcontract_mo_links_company_isolation     ON manufacturing.subcontract_mo_links;
DROP POLICY IF EXISTS unbuild_orders_company_isolation           ON manufacturing.unbuild_orders;
DROP POLICY IF EXISTS work_order_items_company_isolation         ON manufacturing.work_order_items;
DROP POLICY IF EXISTS work_orders_company_isolation              ON manufacturing.work_orders;
DROP POLICY IF EXISTS workstation_losses_company_isolation       ON manufacturing.workstation_losses;
DROP POLICY IF EXISTS workstation_productivity_company_isolation ON manufacturing.workstation_productivity;
DROP POLICY IF EXISTS workstations_company_isolation             ON manufacturing.workstations;

DROP INDEX IF EXISTS manufacturing.idx_bom_items_company_id;
DROP INDEX IF EXISTS manufacturing.idx_bom_operations_company_id;
DROP INDEX IF EXISTS manufacturing.idx_boms_company_id_bom_code;
DROP INDEX IF EXISTS manufacturing.idx_boms_company_id_item_id_status;
DROP INDEX IF EXISTS manufacturing.idx_boms_company_item_version;
DROP INDEX IF EXISTS manufacturing.idx_category_costing_defaults_company_category;
DROP INDEX IF EXISTS manufacturing.idx_category_costing_defaults_company_id_product_category_id;
DROP INDEX IF EXISTS manufacturing.idx_operations_company_id_status;
DROP INDEX IF EXISTS manufacturing.idx_repair_orders_company_id_repair_number;
DROP INDEX IF EXISTS manufacturing.idx_repair_orders_company_id_status;
DROP INDEX IF EXISTS manufacturing.idx_repair_parts_company_id;
DROP INDEX IF EXISTS manufacturing.idx_repair_tags_company_id_name;
DROP INDEX IF EXISTS manufacturing.idx_subcontract_mo_links_company_id_purchase_order_id;
DROP INDEX IF EXISTS manufacturing.idx_subcontract_mo_links_company_po;
DROP INDEX IF EXISTS manufacturing.idx_unbuild_orders_company_id_status;
DROP INDEX IF EXISTS manufacturing.idx_unbuild_orders_company_id_unbuild_number;
DROP INDEX IF EXISTS manufacturing.idx_work_order_items_company_id;
DROP INDEX IF EXISTS manufacturing.idx_work_orders_company_id_status;
DROP INDEX IF EXISTS manufacturing.idx_work_orders_company_id_work_order_number;
DROP INDEX IF EXISTS manufacturing.idx_workstation_losses_company_id_name;
DROP INDEX IF EXISTS manufacturing.idx_workstation_productivity_company_id;
DROP INDEX IF EXISTS manufacturing.idx_workstations_company_id_status;

ALTER TABLE manufacturing.bom_byproducts           DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.bom_items                DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.bom_operations           DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.bom_subcontractors       DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.boms                     DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.category_costing_defaults DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.job_cards                DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.operations               DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.repair_orders            DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.repair_parts             DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.repair_tags              DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.subcontract_mo_links     DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.unbuild_orders           DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.work_order_items         DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.work_orders              DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.workstation_losses       DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.workstation_productivity DROP COLUMN IF EXISTS company_id;
ALTER TABLE manufacturing.workstations             DROP COLUMN IF EXISTS company_id;
