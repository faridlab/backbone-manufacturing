-- Repair family: repair orders (hand-set 5-state chain), part lines (add/remove/recycle),
-- and per-company tags. NO fees models, NO quarantine location — a removed part's destination
-- is the inventory-LOSS account, never a Location row. The tags unique (company_id, name) is
-- the family's sole DB-level guard (G-MEX7, enforcement: db). RLS lands in
-- 20260901110300_rls_fence_new_tables.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'repair_status') THEN
        CREATE TYPE repair_status AS ENUM ('draft', 'confirmed', 'under_repair', 'done', 'cancel');
    END IF;
END
$$;
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'repair_line_type') THEN
        CREATE TYPE repair_line_type AS ENUM ('add', 'remove', 'recycle');
    END IF;
END
$$;

CREATE SCHEMA IF NOT EXISTS manufacturing;

CREATE TABLE IF NOT EXISTS manufacturing.repair_orders (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    repair_number TEXT NOT NULL,
    item_id UUID NOT NULL,
    product_category_id UUID,
    quantity NUMERIC(18, 4) NOT NULL CHECK (quantity > 0),
    status repair_status NOT NULL DEFAULT 'draft',
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id),
    CONSTRAINT chk_repair_orders_number_len CHECK (char_length(repair_number) <= 40)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_repair_orders_company_id_repair_number ON manufacturing.repair_orders (company_id, repair_number) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_repair_orders_company_id_status ON manufacturing.repair_orders (company_id, status);

CREATE INDEX IF NOT EXISTS idx_repair_orders_metadata_gin ON manufacturing.repair_orders USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_repair_orders_metadata_deleted_at ON manufacturing.repair_orders ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.repair_orders_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS repair_orders_insert_audit ON manufacturing.repair_orders;
CREATE TRIGGER repair_orders_insert_audit BEFORE INSERT ON manufacturing.repair_orders
    FOR EACH ROW EXECUTE FUNCTION manufacturing.repair_orders_audit_timestamp();
DROP TRIGGER IF EXISTS repair_orders_update_audit ON manufacturing.repair_orders;
CREATE TRIGGER repair_orders_update_audit BEFORE UPDATE ON manufacturing.repair_orders
    FOR EACH ROW EXECUTE FUNCTION manufacturing.repair_orders_audit_timestamp();

CREATE TABLE IF NOT EXISTS manufacturing.repair_parts (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    repair_order_id UUID NOT NULL REFERENCES manufacturing.repair_orders(id),
    item_id UUID NOT NULL,
    warehouse_id UUID,
    line_type repair_line_type NOT NULL,
    quantity NUMERIC(18, 4) NOT NULL CHECK (quantity > 0),
    rate NUMERIC(18, 2) NOT NULL DEFAULT 0 CHECK (rate >= 0),
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS idx_repair_parts_repair_order_id ON manufacturing.repair_parts (repair_order_id);
CREATE INDEX IF NOT EXISTS idx_repair_parts_company_id ON manufacturing.repair_parts (company_id);

CREATE INDEX IF NOT EXISTS idx_repair_parts_metadata_gin ON manufacturing.repair_parts USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_repair_parts_metadata_deleted_at ON manufacturing.repair_parts ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.repair_parts_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS repair_parts_insert_audit ON manufacturing.repair_parts;
CREATE TRIGGER repair_parts_insert_audit BEFORE INSERT ON manufacturing.repair_parts
    FOR EACH ROW EXECUTE FUNCTION manufacturing.repair_parts_audit_timestamp();
DROP TRIGGER IF EXISTS repair_parts_update_audit ON manufacturing.repair_parts;
CREATE TRIGGER repair_parts_update_audit BEFORE UPDATE ON manufacturing.repair_parts
    FOR EACH ROW EXECUTE FUNCTION manufacturing.repair_parts_audit_timestamp();

CREATE TABLE IF NOT EXISTS manufacturing.repair_tags (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    name TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id),
    CONSTRAINT chk_repair_tags_name_len CHECK (char_length(name) <= 140)
);

-- G-MEX7: the family's sole DB-level guard. Tenant-scoped unique (company_id, name) — a
-- deliberate deviation from the upstream GLOBAL unique(name), which would collide across
-- tenants.
CREATE UNIQUE INDEX IF NOT EXISTS idx_repair_tags_company_id_name ON manufacturing.repair_tags (company_id, name) WHERE (metadata->>'deleted_at') IS NULL;

CREATE INDEX IF NOT EXISTS idx_repair_tags_metadata_gin ON manufacturing.repair_tags USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_repair_tags_metadata_deleted_at ON manufacturing.repair_tags ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.repair_tags_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS repair_tags_insert_audit ON manufacturing.repair_tags;
CREATE TRIGGER repair_tags_insert_audit BEFORE INSERT ON manufacturing.repair_tags
    FOR EACH ROW EXECUTE FUNCTION manufacturing.repair_tags_audit_timestamp();
DROP TRIGGER IF EXISTS repair_tags_update_audit ON manufacturing.repair_tags;
CREATE TRIGGER repair_tags_update_audit BEFORE UPDATE ON manufacturing.repair_tags
    FOR EACH ROW EXECUTE FUNCTION manufacturing.repair_tags_audit_timestamp();
