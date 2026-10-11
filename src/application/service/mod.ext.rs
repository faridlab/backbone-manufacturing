// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod manufacturing_gl;
pub mod manufacturing_ports;
pub mod manufacturing_events;
pub mod manufacturing_write_service;
// The write surface, chunked: each is an `impl ManufacturingWriteService` block over the vocabulary
// that stays in `manufacturing_write_service` (so the `manufacturing_write_service::{NewBom, ...}`
// import paths are unchanged).
pub mod manufacturing_bom_definition;
pub mod manufacturing_work_order;
pub mod manufacturing_execution;
pub mod manufacturing_job_card;
pub mod manufacturing_unbuild;
pub mod manufacturing_repair;
pub mod manufacturing_workcenter;
pub mod manufacturing_subcontract;
pub use manufacturing_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostLine, GlPostRejected, GlPostSink,
};
pub use manufacturing_ports::{
    CostPosture, FinishedReceipt, InventoryPort, InventoryRejected, IssueLine, IssuedLineValue,
    IssueAck, MaterialIssue, ReceiptByproduct, RepairAvailability, RepairLeg, UnbuildReversal,
};
pub use manufacturing_events::{
    ConversionCharged, FinishedGoodsReceived, LoggingSink, ManufacturingEvent,
    ManufacturingEventSink, MaterialsConsumed, RepairCompleted, SubcontractMoMinted,
    UnbuildExecuted, WorkOrderCancelled, WorkOrderCompleted, WorkOrderConfirmed,
};
pub use manufacturing_subcontract::{SubcontractReceiptEvent, SubcontractReceiptLine};
pub use manufacturing_write_service::{
    ConsumeOutcome, ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem,
    NewBomOperation, NewJobCard, NewProductivity, NewRepairOrder, NewRepairPart, NewUnbuild,
    NewWorkOrder, NewWorkstationLoss, OeeReport, ReceiveByproductLine, ReceiveFinishedOrder,
    ReceiveOutcome, RepairEndOutcome, UnbuildOutcome,
};
