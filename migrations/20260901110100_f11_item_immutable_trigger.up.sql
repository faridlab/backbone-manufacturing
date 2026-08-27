-- F11 backstop: the product of a work order is immutable once the order leaves draft.
-- A silent re-point would strand already-consumed components against the wrong product and
-- desynchronise the work-order quantity from the stock-move estate, so the database refuses
-- the change loudly instead of letting the application layer be the only guard.

CREATE OR REPLACE FUNCTION manufacturing.work_orders_item_immutable() RETURNS trigger AS $$
BEGIN
    IF NEW.item_id IS DISTINCT FROM OLD.item_id AND OLD.status <> 'draft' THEN
        RAISE EXCEPTION 'work_order % item_id is immutable after draft (current state: %)', OLD.id, OLD.status;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS work_orders_item_immutable ON manufacturing.work_orders;
CREATE TRIGGER work_orders_item_immutable BEFORE UPDATE ON manufacturing.work_orders
    FOR EACH ROW EXECUTE FUNCTION manufacturing.work_orders_item_immutable();
