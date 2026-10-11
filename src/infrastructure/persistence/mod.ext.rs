// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-written manufacturing SQL repositories (declared `user_owned` in metaphor.codegen.yaml).
// Each COMBINES several related tables behind the write path's gates (unbuild, repair family,
// workcenter/OEE family, costing defaults, subcontract links); the per-entity repositories the
// generator emits above remain the plain CRUD stack. Declared here — inside the marker — so a
// regen keeps them wired.
pub mod unbuild_repository;
pub mod repair_repository;
pub mod workcenter_repository;
pub mod costing_defaults_repository;
pub mod subcontract_link_repository;
pub use unbuild_repository::UnbuildRepository;
pub use repair_repository::RepairRepository;
pub use workcenter_repository::WorkcenterRepository;
pub use costing_defaults_repository::CostingDefaultsRepository;
pub use subcontract_link_repository::SubcontractLinkRepository;
// The hand-written manufacturing SQL's parameter/projection types. Their repositories are declared
// `user_owned` in metaphor.codegen.yaml; these types mirror COLUMNS, not entities.
pub use bom_repository::NewBomRow;
pub use bom_item_repository::{BomComponentRow, NewBomItemRow};
pub use bom_operation_repository::NewBomOperationRow;
pub use work_order_repository::{ConfirmSourceRow, NewWorkOrderRow, WorkOrderRow};
pub use work_order_item_repository::{NewWorkOrderItemRow, WorkOrderRequirementRow};
pub use job_card_repository::{JobCardCompletionRow, NewJobCardRow};
pub use bom_byproduct_repository::{BomByproductRow, NewBomByproductRow};
pub use bom_subcontractor_repository::NewBomSubcontractorRow;
pub use unbuild_repository::{NewUnbuildRow, UnbuildExecuteRow};
pub use repair_repository::{
    NewRepairOrderRow, NewRepairPartRow, RepairOrderRow, RepairPartRow,
};
pub use workcenter_repository::{
    NewWorkstationLossRow, NewWorkstationProductivityRow, OeeBucketRow,
};
pub use costing_defaults_repository::{CostingDefaultsAccounts, NewCostingDefaultsRow};
pub use subcontract_link_repository::NewSubcontractLinkRow;
