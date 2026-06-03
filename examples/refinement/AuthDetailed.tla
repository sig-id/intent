\* Hand-written DETAILED model for the refinement bridge demo.
\*
\* This is the layer the Intent compiler does NOT generate: it carries the real
\* implementation logic. The generated harness (Auth_LoginFlow_Refinement.tla)
\* EXTENDS this module and checks that every step here projects onto a step of
\* the abstract FSM compiled from auth.intent.
\*
\* It must define, for the grounding to be met:
\*   - AbsLoginState  (the `state` abstraction function)
\*   - pw_ok          (grounds guard atom `password_valid`)
\*   - acct_active    (grounds guard atom `account_active`)
\* and the usual Init / Next / Spec / vars so the harness can drive it.
---- MODULE AuthDetailed ----
EXTENDS Naturals, Sequences

VARIABLES cstate, pw, acct
vars == <<cstate, pw, acct>>

\* Guard groundings: detailed facts the abstract guard atoms stand for.
pw_ok == pw
acct_active == acct

\* Abstraction function: detailed concrete state -> abstract FSM state name.
AbsLoginState ==
    CASE cstate = "start"    -> "idle"
      [] cstate = "checking" -> "verifying"
      [] cstate = "granted"  -> "authenticated"
      [] cstate = "refused"  -> "denied"
      [] OTHER               -> "idle"

Init ==
    /\ cstate = "start"
    /\ pw \in BOOLEAN
    /\ acct \in BOOLEAN

Submit ==
    /\ cstate = "start"
    /\ cstate' = "checking"
    /\ UNCHANGED <<pw, acct>>

Grant ==
    /\ cstate = "checking"
    /\ (pw /\ acct)
    /\ cstate' = "granted"
    /\ UNCHANGED <<pw, acct>>

Refuse ==
    /\ cstate = "checking"
    /\ ~(pw /\ acct)
    /\ cstate' = "refused"
    /\ UNCHANGED <<pw, acct>>

Next == Submit \/ Grant \/ Refuse \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars
====
