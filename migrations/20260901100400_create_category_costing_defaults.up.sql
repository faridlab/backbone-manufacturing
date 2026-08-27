-- Per-company, per-product-category costing defaults (strict fence — defaults are company
-- decisions). Resolution order everywhere: explicit override -> category default -> MissingAccount
-- refusal; never a hardcoded fallback. RLS lands in 20260901110300_rls_fence_new_tables.

CREATE SCHEMA IF NOT EXISTS manufacturing;

CREATE TABLE IF NOT EXISTS manufacturing.category_costing_defaults (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    product_category_id UUID NOT NULL,
    wip_account_id UUID,
    fg_account_id UUID,
    raw_material_account_id UUID,
    conversion_cost_account_id UUID,
    subcontract_interim_account_id UUID,
    cost_variance_account_id UUID,
    inventory_loss_account_id UUID,
    repair_expense_account_id UUID,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_category_costing_defaults_company_category ON manufacturing.category_costing_defaults (company_id, product_category_id) WHERE (metadata->>'deleted_at') IS NULL;

CREATE INDEX IF NOT EXISTS idx_category_costing_defaults_metadata_gin ON manufacturing.category_costing_defaults USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_category_costing_defaults_metadata_deleted_at ON manufacturing.category_costing_defaults ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.category_costing_defaults_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS category_costing_defaults_insert_audit ON manufacturing.category_costing_defaults;
CREATE TRIGGER category_costing_defaults_insert_audit BEFORE INSERT ON manufacturing.category_costing_defaults
    FOR EACH ROW EXECUTE FUNCTION manufacturing.category_costing_defaults_audit_timestamp();
DROP TRIGGER IF EXISTS category_costing_defaults_update_audit ON manufacturing.category_costing_defaults;
CREATE TRIGGER category_costing_defaults_update_audit BEFORE UPDATE ON manufacturing.category_costing_defaults
    FOR EACH ROW EXECUTE FUNCTION manufacturing.category_costing_defaults_audit_timestamp();
