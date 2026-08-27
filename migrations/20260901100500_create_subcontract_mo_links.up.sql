-- Subcontract purchase-order -> work-order link table: the idempotency backstop for
-- subcontract receipt events. One hidden work order per (company, purchase order); a
-- replayed event returns the existing work order. RLS lands in
-- 20260901110300_rls_fence_new_tables.

CREATE SCHEMA IF NOT EXISTS manufacturing;

CREATE TABLE IF NOT EXISTS manufacturing.subcontract_mo_links (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    purchase_order_id UUID NOT NULL,
    work_order_id UUID NOT NULL REFERENCES manufacturing.work_orders(id),
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_subcontract_mo_links_company_po ON manufacturing.subcontract_mo_links (company_id, purchase_order_id) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_subcontract_mo_links_work_order_id ON manufacturing.subcontract_mo_links (work_order_id);

CREATE INDEX IF NOT EXISTS idx_subcontract_mo_links_metadata_gin ON manufacturing.subcontract_mo_links USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_subcontract_mo_links_metadata_deleted_at ON manufacturing.subcontract_mo_links ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.subcontract_mo_links_audit_timestamp() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{created_at}', to_jsonb(NOW()));
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    ELSIF TG_OP = 'UPDATE' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS subcontract_mo_links_insert_audit ON manufacturing.subcontract_mo_links;
CREATE TRIGGER subcontract_mo_links_insert_audit BEFORE INSERT ON manufacturing.subcontract_mo_links
    FOR EACH ROW EXECUTE FUNCTION manufacturing.subcontract_mo_links_audit_timestamp();
DROP TRIGGER IF EXISTS subcontract_mo_links_update_audit ON manufacturing.subcontract_mo_links;
CREATE TRIGGER subcontract_mo_links_update_audit BEFORE UPDATE ON manufacturing.subcontract_mo_links
    FOR EACH ROW EXECUTE FUNCTION manufacturing.subcontract_mo_links_audit_timestamp();
