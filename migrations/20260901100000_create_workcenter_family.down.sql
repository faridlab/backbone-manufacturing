-- Reverse the workcenter family create.
DROP TRIGGER IF EXISTS workstation_productivity_update_audit ON manufacturing.workstation_productivity;
DROP TRIGGER IF EXISTS workstation_productivity_insert_audit ON manufacturing.workstation_productivity;
DROP FUNCTION IF EXISTS manufacturing.workstation_productivity_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.workstation_productivity;

DROP TRIGGER IF EXISTS workstation_losses_update_audit ON manufacturing.workstation_losses;
DROP TRIGGER IF EXISTS workstation_losses_insert_audit ON manufacturing.workstation_losses;
DROP FUNCTION IF EXISTS manufacturing.workstation_losses_audit_timestamp();
DROP TABLE IF EXISTS manufacturing.workstation_losses;

DROP TYPE IF EXISTS loss_type;
