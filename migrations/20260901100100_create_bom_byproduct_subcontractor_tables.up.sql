-- BoM byproduct lines (cost-share split) + read-only subcontractor references.
-- Master-data family: nullable company_id (shared_blank) — the NULL row is a first-class
-- shared row. The row CHECK carries G-B4's per-row half (0 <= cost_share <= 100); the
-- sum-across-byproducts half is service-enforced at BoM authoring. RLS lands in
-- 20260901110300_rls_fence_new_tables.

CREATE SCHEMA IF NOT EXISTS manufacturing;

CREATE TABLE IF NOT EXISTS manufacturing.bom_byproducts (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID,
    bom_id UUID NOT NULL REFERENCES manufacturing.boms(id),
    item_id UUID NOT NULL,
    product_category_id UUID,
    quantity NUMERIC(18, 4) NOT NULL CHECK (quantity > 0),
    cost_share NUMERIC(5, 2) NOT NULL DEFAULT 0 CHECK (cost_share >= 0 AND cost_share <= 100),
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS idx_bom_byproducts_bom_id ON manufacturing.bom_byproducts (bom_id);
CREATE INDEX IF NOT EXISTS idx_bom_byproducts_metadata_gin ON manufacturing.bom_byproducts USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_bom_byproducts_metadata_deleted_at ON manufacturing.bom_byproducts ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.bom_byproducts_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS bom_byproducts_insert_audit ON manufacturing.bom_byproducts;
CREATE TRIGGER bom_byproducts_insert_audit BEFORE INSERT ON manufacturing.bom_byproducts
    FOR EACH ROW EXECUTE FUNCTION manufacturing.bom_byproducts_audit_timestamp();
DROP TRIGGER IF EXISTS bom_byproducts_update_audit ON manufacturing.bom_byproducts;
CREATE TRIGGER bom_byproducts_update_audit BEFORE UPDATE ON manufacturing.bom_byproducts
    FOR EACH ROW EXECUTE FUNCTION manufacturing.bom_byproducts_audit_timestamp();

CREATE TABLE IF NOT EXISTS manufacturing.bom_subcontractors (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID,
    bom_id UUID NOT NULL REFERENCES manufacturing.boms(id),
    partner_id UUID NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_bom_subcontractors_bom_id_partner_id ON manufacturing.bom_subcontractors (bom_id, partner_id) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_bom_subcontractors_metadata_gin ON manufacturing.bom_subcontractors USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_bom_subcontractors_metadata_deleted_at ON manufacturing.bom_subcontractors ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.bom_subcontractors_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS bom_subcontractors_insert_audit ON manufacturing.bom_subcontractors;
CREATE TRIGGER bom_subcontractors_insert_audit BEFORE INSERT ON manufacturing.bom_subcontractors
    FOR EACH ROW EXECUTE FUNCTION manufacturing.bom_subcontractors_audit_timestamp();
DROP TRIGGER IF EXISTS bom_subcontractors_update_audit ON manufacturing.bom_subcontractors;
CREATE TRIGGER bom_subcontractors_update_audit BEFORE UPDATE ON manufacturing.bom_subcontractors
    FOR EACH ROW EXECUTE FUNCTION manufacturing.bom_subcontractors_audit_timestamp();
