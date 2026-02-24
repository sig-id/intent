---- MODULE ProducerConsumer_System ----
EXTENDS Naturals, Sequences, FiniteSets, Apalache, TLC

VARIABLES
    \* @type: Str;
    Producer_state,
    \* @type: Int;
    Producer_pc,
    \* @type: Seq(Str);
    Producer_history,
    \* @type: Str;
    Consumer_state,
    \* @type: Int;
    Consumer_pc,
    \* @type: Seq(Str);
    Consumer_history,
    \* @type: Seq({ type: Str, arg0: Int });
    Channel_queue

vars == <<Producer_state, Producer_pc, Producer_history, Consumer_state, Consumer_pc, Consumer_history, Channel_queue>>

Init ==
    /\ Producer_state = "idle"
    /\ Producer_pc = 0
    /\ Producer_history = <<>>
    /\ Consumer_state = "waiting"
    /\ Consumer_pc = 0
    /\ Consumer_history = <<>>
    /\ Channel_queue = <<>>

\* Transition actions

Producer_idle_produce ==
    /\ Producer_state = "idle"
    /\ Producer_state' = "sent"
    /\ Producer_pc' = Producer_pc + 1
    /\ Producer_history' = Append(Producer_history, Producer_state)
    /\ Channel_queue' = Append(Channel_queue, [type |-> "Data", arg0 |-> 42])
    /\ UNCHANGED <<Consumer_history, Consumer_pc, Consumer_state>>

Consumer_waiting_consume ==
    /\ Consumer_state = "waiting"
    /\ Consumer_state' = "received"
    /\ Consumer_pc' = Consumer_pc + 1
    /\ Consumer_history' = Append(Consumer_history, Consumer_state)
    /\ Len(Channel_queue) > 0
    /\ Channel_queue' = Tail(Channel_queue)
    /\ UNCHANGED <<Producer_state, Producer_history, Producer_pc>>

Next ==
    \/ Producer_idle_produce
    \/ Consumer_waiting_consume
    \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars

\* Type invariant
TypeOK ==
    /\ Producer_state \in {"idle", "sent"}
    /\ Producer_pc \in Nat
    /\ Consumer_state \in {"waiting", "received"}
    /\ Consumer_pc \in Nat
    /\ Len(Channel_queue) <= 10

\* History length matches step count
HistoryConsistent ==
    /\ Len(Producer_history) = Producer_pc
    /\ Len(Consumer_history) = Consumer_pc

====
