-- Unbuild orders (transactional, strict company fence). The 2-state plain enum is created
-- unqualified (repo convention). RLS lands in 20260901110300_rls_fence_new_tables.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'unbuild_status') THEN
        CREATE TYPE unbuild_status AS ENUM ('draft', 'done');
    END IF;
END
$$;

CREATE SCHEMA IF NOT EXISTS manufacturing;

CREATE TABLE IF NOT EXISTS manufacturing.unbuild_orders (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    unbuild_number TEXT NOT NULL,
    work_order_id UUID NOT NULL REFERENCES manufacturing.work_orders(id),
    item_id UUID NOT NULL,
    quantity NUMERIC(18, 4) NOT NULL CHECK (quantity > 0),
    status unbuild_status NOT NULL DEFAULT 'draft',
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id),
    CONSTRAINT chk_unbuild_orders_number_len CHECK (char_length(unbuild_number) <= 40)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_unbuild_orders_company_id_unbuild_number ON manufacturing.unbuild_orders (company_id, unbuild_number) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_unbuild_orders_work_order_id ON manufacturing.unbuild_orders (work_order_id);
CREATE INDEX IF NOT EXISTS idx_unbuild_orders_company_id_status ON manufacturing.unbuild_orders (company_id, status);

CREATE INDEX IF NOT EXISTS idx_unbuild_orders_metadata_gin ON manufacturing.unbuild_orders USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_unbuild_orders_metadata_deleted_at ON manufacturing.unbuild_orders ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.unbuild_orders_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS unbuild_orders_insert_audit ON manufacturing.unbuild_orders;
CREATE TRIGGER unbuild_orders_insert_audit BEFORE INSERT ON manufacturing.unbuild_orders
    FOR EACH ROW EXECUTE FUNCTION manufacturing.unbuild_orders_audit_timestamp();
DROP TRIGGER IF EXISTS unbuild_orders_update_audit ON manufacturing.unbuild_orders;
CREATE TRIGGER unbuild_orders_update_audit BEFORE UPDATE ON manufacturing.unbuild_orders
    FOR EACH ROW EXECUTE FUNCTION manufacturing.unbuild_orders_audit_timestamp();
