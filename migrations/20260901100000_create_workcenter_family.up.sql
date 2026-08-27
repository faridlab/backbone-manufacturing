-- Workcenter family: named productivity-loss reasons (master data) + timed productivity
-- records (transactions). Duration is read-side computed (date_end - date_start) — nothing is
-- stored or maintained by a background job. The RLS fence for both tables lands in
-- 20260901110300_rls_fence_new_tables.

-- Create loss_type enum type (unqualified, public schema — repo convention)
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'loss_type') THEN
        CREATE TYPE loss_type AS ENUM ('productive', 'availability', 'performance', 'quality');
    END IF;
END
$$;

CREATE SCHEMA IF NOT EXISTS manufacturing;

CREATE TABLE IF NOT EXISTS manufacturing.workstation_losses (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID,
    name TEXT NOT NULL,
    loss_type loss_type NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id),
    CONSTRAINT chk_workstation_losses_name_len CHECK (char_length(name) <= 140)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_workstation_losses_company_id_name ON manufacturing.workstation_losses (company_id, name) NULLS NOT DISTINCT WHERE (metadata->>'deleted_at') IS NULL;

CREATE INDEX IF NOT EXISTS idx_workstation_losses_metadata_gin ON manufacturing.workstation_losses USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_workstation_losses_metadata_deleted_at ON manufacturing.workstation_losses ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.workstation_losses_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS workstation_losses_insert_audit ON manufacturing.workstation_losses;
CREATE TRIGGER workstation_losses_insert_audit BEFORE INSERT ON manufacturing.workstation_losses
    FOR EACH ROW EXECUTE FUNCTION manufacturing.workstation_losses_audit_timestamp();
DROP TRIGGER IF EXISTS workstation_losses_update_audit ON manufacturing.workstation_losses;
CREATE TRIGGER workstation_losses_update_audit BEFORE UPDATE ON manufacturing.workstation_losses
    FOR EACH ROW EXECUTE FUNCTION manufacturing.workstation_losses_audit_timestamp();

CREATE TABLE IF NOT EXISTS manufacturing.workstation_productivity (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    workstation_id UUID NOT NULL REFERENCES manufacturing.workstations(id),
    job_card_id UUID REFERENCES manufacturing.job_cards(id),
    loss_id UUID NOT NULL REFERENCES manufacturing.workstation_losses(id),
    date_start TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    date_end TIMESTAMPTZ,
    description TEXT,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id),
    CONSTRAINT chk_workstation_productivity_date_order CHECK (date_end IS NULL OR date_end >= date_start)
);

CREATE INDEX IF NOT EXISTS idx_workstation_productivity_workstation_date ON manufacturing.workstation_productivity (workstation_id, date_start);
CREATE INDEX IF NOT EXISTS idx_workstation_productivity_loss_id ON manufacturing.workstation_productivity (loss_id);
CREATE INDEX IF NOT EXISTS idx_workstation_productivity_company_id ON manufacturing.workstation_productivity (company_id);

CREATE INDEX IF NOT EXISTS idx_workstation_productivity_metadata_gin ON manufacturing.workstation_productivity USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_workstation_productivity_metadata_deleted_at ON manufacturing.workstation_productivity ((metadata->>'deleted_at'));

CREATE OR REPLACE FUNCTION manufacturing.workstation_productivity_audit_timestamp() RETURNS trigger AS $$
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

DROP TRIGGER IF EXISTS workstation_productivity_insert_audit ON manufacturing.workstation_productivity;
CREATE TRIGGER workstation_productivity_insert_audit BEFORE INSERT ON manufacturing.workstation_productivity
    FOR EACH ROW EXECUTE FUNCTION manufacturing.workstation_productivity_audit_timestamp();
DROP TRIGGER IF EXISTS workstation_productivity_update_audit ON manufacturing.workstation_productivity;
CREATE TRIGGER workstation_productivity_update_audit BEFORE UPDATE ON manufacturing.workstation_productivity
    FOR EACH ROW EXECUTE FUNCTION manufacturing.workstation_productivity_audit_timestamp();
