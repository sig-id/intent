---- MODULE OrderPayment_System ----
EXTENDS Naturals, Sequences, FiniteSets, Apalache, TLC

VARIABLES
    \* @type: Str;
    OrderProcessing_state,
    \* @type: Int;
    OrderProcessing_pc,
    \* @type: Seq(Str);
    OrderProcessing_history,
    \* @type: Int;
    OrderProcessing_orderID,
    \* @type: Str;
    PaymentProcessing_state,
    \* @type: Int;
    PaymentProcessing_pc,
    \* @type: Seq(Str);
    PaymentProcessing_history,
    \* @type: Int;
    PaymentProcessing_processingOrderId,
    \* @type: Seq({ type: Str, arg0: Int, arg1: Int });
    PaymentService_queue

vars == <<OrderProcessing_state, OrderProcessing_pc, OrderProcessing_history, OrderProcessing_orderID, PaymentProcessing_state, PaymentProcessing_pc, PaymentProcessing_history, PaymentProcessing_processingOrderId, PaymentService_queue>>

Init ==
    /\ OrderProcessing_state = "pending"
    /\ OrderProcessing_pc = 0
    /\ OrderProcessing_history = <<>>
    /\ OrderProcessing_orderID = 0
    /\ PaymentProcessing_state = "idle"
    /\ PaymentProcessing_pc = 0
    /\ PaymentProcessing_history = <<>>
    /\ PaymentProcessing_processingOrderId = 0
    /\ PaymentService_queue = <<>>

\* Transition actions

OrderProcessing_pending_create_order ==
    /\ OrderProcessing_state = "pending"
    /\ OrderProcessing_state' = "payment_requested"
    /\ OrderProcessing_pc' = OrderProcessing_pc + 1
    /\ OrderProcessing_history' = Append(OrderProcessing_history, OrderProcessing_state)
    /\ OrderProcessing_orderID'  = 123
    /\ PaymentService_queue' = Append(PaymentService_queue, [type |-> "PaymentRequest", arg0 |-> 123, arg1 |-> 100])
    /\ UNCHANGED <<PaymentProcessing_pc, PaymentProcessing_processingOrderId, PaymentProcessing_state, PaymentProcessing_history>>

OrderProcessing_payment_requested_payment_success ==
    /\ OrderProcessing_state = "payment_requested"
    /\ OrderProcessing_state' = "completed"
    /\ OrderProcessing_pc' = OrderProcessing_pc + 1
    /\ OrderProcessing_history' = Append(OrderProcessing_history, OrderProcessing_state)
    /\ Len(PaymentService_queue) > 0
    /\ PaymentService_queue' = Tail(PaymentService_queue)
    /\ UNCHANGED <<PaymentProcessing_pc, PaymentProcessing_processingOrderId, OrderProcessing_orderID, PaymentProcessing_state, PaymentProcessing_history>>

OrderProcessing_payment_requested_payment_failure ==
    /\ OrderProcessing_state = "payment_requested"
    /\ OrderProcessing_state' = "failed"
    /\ OrderProcessing_pc' = OrderProcessing_pc + 1
    /\ OrderProcessing_history' = Append(OrderProcessing_history, OrderProcessing_state)
    /\ Len(PaymentService_queue) > 0
    /\ PaymentService_queue' = Tail(PaymentService_queue)
    /\ UNCHANGED <<PaymentProcessing_state, PaymentProcessing_pc, OrderProcessing_orderID, PaymentProcessing_processingOrderId, PaymentProcessing_history>>

PaymentProcessing_idle_receive_payment ==
    /\ PaymentProcessing_state = "idle"
    /\ PaymentProcessing_state' = "processing"
    /\ PaymentProcessing_pc' = PaymentProcessing_pc + 1
    /\ PaymentProcessing_history' = Append(PaymentProcessing_history, PaymentProcessing_state)
    /\ Len(PaymentService_queue) > 0
    /\ PaymentService_queue' = Tail(PaymentService_queue)
    /\ UNCHANGED <<OrderProcessing_pc, OrderProcessing_orderID, OrderProcessing_state, OrderProcessing_history, PaymentProcessing_processingOrderId>>

PaymentProcessing_processing_confirm ==
    /\ PaymentProcessing_state = "processing"
    /\ PaymentProcessing_state' = "confirmed"
    /\ PaymentProcessing_pc' = PaymentProcessing_pc + 1
    /\ PaymentProcessing_history' = Append(PaymentProcessing_history, PaymentProcessing_state)
    /\ PaymentService_queue' = Append(PaymentService_queue, [type |-> "PaymentConfirmed", arg0 |-> 123, arg1 |-> 1])
    /\ UNCHANGED <<OrderProcessing_state, PaymentProcessing_processingOrderId, OrderProcessing_orderID, OrderProcessing_pc, OrderProcessing_history>>

Next ==
    \/ OrderProcessing_pending_create_order
    \/ OrderProcessing_payment_requested_payment_success
    \/ OrderProcessing_payment_requested_payment_failure
    \/ PaymentProcessing_idle_receive_payment
    \/ PaymentProcessing_processing_confirm
    \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars

\* Type invariant
TypeOK ==
    /\ OrderProcessing_state \in {"pending", "payment_requested", "completed", "failed"}
    /\ OrderProcessing_pc \in Nat
    /\ PaymentProcessing_state \in {"idle", "processing", "confirmed"}
    /\ PaymentProcessing_pc \in Nat
    /\ Len(PaymentService_queue) <= 10

\* History length matches step count
HistoryConsistent ==
    /\ Len(OrderProcessing_history) = OrderProcessing_pc
    /\ Len(PaymentProcessing_history) = PaymentProcessing_pc

====
