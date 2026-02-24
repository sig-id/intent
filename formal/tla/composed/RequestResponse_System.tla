---- MODULE RequestResponse_System ----
EXTENDS Naturals, Sequences, FiniteSets, Apalache, TLC

VARIABLES
    \* @type: Str;
    Client_state,
    \* @type: Int;
    Client_pc,
    \* @type: Seq(Str);
    Client_history,
    \* @type: Int;
    Client_requestId,
    \* @type: Str;
    Server_state,
    \* @type: Int;
    Server_pc,
    \* @type: Seq(Str);
    Server_history,
    \* @type: Int;
    Server_queryId,
    \* @type: Seq({ type: Str, arg0: Int });
    RequestChannel_queue,
    \* @type: Seq({ type: Str, arg0: Int, arg1: Int });
    ResponseChannel_queue

vars == <<Client_state, Client_pc, Client_history, Client_requestId, Server_state, Server_pc, Server_history, Server_queryId, RequestChannel_queue, ResponseChannel_queue>>

Init ==
    /\ Client_state = "idle"
    /\ Client_pc = 0
    /\ Client_history = <<>>
    /\ Client_requestId = 0
    /\ Server_state = "listening"
    /\ Server_pc = 0
    /\ Server_history = <<>>
    /\ Server_queryId = 0
    /\ RequestChannel_queue = <<>>
    /\ ResponseChannel_queue = <<>>

\* Transition actions

Client_idle_send_request ==
    /\ Client_state = "idle"
    /\ Client_state' = "waiting"
    /\ Client_pc' = Client_pc + 1
    /\ Client_history' = Append(Client_history, Client_state)
    /\ Client_requestId'  = 1
    /\ RequestChannel_queue' = Append(RequestChannel_queue, [type |-> "Query", arg0 |-> 1])
    /\ UNCHANGED <<Server_state, ResponseChannel_queue, Server_queryId, Server_history, Server_pc>>

Client_waiting_receive_response ==
    /\ Client_state = "waiting"
    /\ Client_state' = "done"
    /\ Client_pc' = Client_pc + 1
    /\ Client_history' = Append(Client_history, Client_state)
    /\ Len(ResponseChannel_queue) > 0
    /\ ResponseChannel_queue' = Tail(ResponseChannel_queue)
    /\ UNCHANGED <<Server_history, Client_requestId, Server_state, Server_queryId, RequestChannel_queue, Server_pc>>

Server_listening_receive_query ==
    /\ Server_state = "listening"
    /\ Server_state' = "processing"
    /\ Server_pc' = Server_pc + 1
    /\ Server_history' = Append(Server_history, Server_state)
    /\ Len(RequestChannel_queue) > 0
    /\ RequestChannel_queue' = Tail(RequestChannel_queue)
    /\ UNCHANGED <<Server_queryId, Client_requestId, Client_state, Client_pc, Client_history, ResponseChannel_queue>>

Server_processing_send_result ==
    /\ Server_state = "processing"
    /\ Server_state' = "responded"
    /\ Server_pc' = Server_pc + 1
    /\ Server_history' = Append(Server_history, Server_state)
    /\ ResponseChannel_queue' = Append(ResponseChannel_queue, [type |-> "Result", arg0 |-> 1, arg1 |-> 42])
    /\ UNCHANGED <<Server_queryId, RequestChannel_queue, Client_state, Client_pc, Client_history, Client_requestId>>

Next ==
    \/ Client_idle_send_request
    \/ Client_waiting_receive_response
    \/ Server_listening_receive_query
    \/ Server_processing_send_result
    \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars

\* Type invariant
TypeOK ==
    /\ Client_state \in {"idle", "waiting", "done"}
    /\ Client_pc \in Nat
    /\ Server_state \in {"listening", "processing", "responded"}
    /\ Server_pc \in Nat
    /\ Len(RequestChannel_queue) <= 10
    /\ Len(ResponseChannel_queue) <= 10

\* History length matches step count
HistoryConsistent ==
    /\ Len(Client_history) = Client_pc
    /\ Len(Server_history) = Server_pc

====
