-- Reverse the F11 item-immutability backstop.
DROP TRIGGER IF EXISTS work_orders_item_immutable ON manufacturing.work_orders;
DROP FUNCTION IF EXISTS manufacturing.work_orders_item_immutable();
