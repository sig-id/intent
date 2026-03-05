# Effect Block Semantics in Intent

## Overview

Intent uses **declarative semantics** for effect blocks, not imperative/sequential semantics. This document explains what that means and why this design choice was made.

## The Core Principle

In an effect block, all statements describe a **single atomic state transition**:

```
CurrentState + EffectBlock → NextState
```

### Variable References

- **Unprimed reads** (e.g., `counter`): Read from **current state**
- **Primed writes** (e.g., `counter'` in TLA+): Write to **next state**

When you write:
```intent
effect {
    x = 5
    send M(val: x)
}
```

This means:
- `x'` = 5 (next state)
- Send message with `x` (current state)

**The order of these statements doesn't matter semantically.**

## Common Misunderstandings

### Misunderstanding #1: Sequential Execution

❌ **Wrong mental model** (imperative):
```intent
effect {
    count = count + 1      // Step 1: increment count
    send Value(n: count)   // Step 2: send new value
}
```

"First increment, then send the new value."

✅ **Correct mental model** (declarative):
```intent
effect {
    count = count + 1      // count' = count + 1
    send Value(n: count)   // Uses current count, not count'
}
```

"Simultaneously: set count' to count+1, and send current count."

### Misunderstanding #2: Variable Shadowing

❌ **Wrong:**
```intent
effect {
    temp = x
    x = y
    y = temp    // Doesn't reference the NEW temp!
}
```

This doesn't swap values. `temp` in the last line reads from current state (whatever it was before), not the newly assigned value.

✅ **Correct:**
```intent
effect {
    x = y  // x' = current y
    y = x  // y' = current x
}
```

No temporary variable needed! Both reads happen from current state, both writes go to next state.

## Practical Guidelines

### 1. Understand: Order Is Irrelevant

The most important thing to internalize: **statement order doesn't matter**

```intent
// These are IDENTICAL:
effect {
    x = 5
    send M(val: x)
}

effect {
    send M(val: x)  // Same! Order irrelevant!
    x = 5
}
```

Both send the **current** value of x and set x' to 5.

### 2. Variable Reads Always See Current State

When you reference a variable in an effect block, it ALWAYS reads from current state:

```intent
effect {
    counter = counter + 1        // counter' = current counter + 1
    send Value(n: counter)       // sends current counter (NOT counter')
}
```

This is **not a bug** - it's the definition of declarative semantics!

### 3. Consequences of Simultaneous Updates

```intent
// If counter = 5 initially:
effect {
    counter = counter + 1        // counter' = 6
    send Value(n: counter)       // sends 5 (not 6!)
}

// To send 6, inline the expression:
effect {
    counter = counter + 1        // counter' = 6
    send Value(n: counter + 1)   // sends 6 (computes from current: 5+1)
}

// Or use a literal if you know the value:
effect {
    counter = 10                 // counter' = 10
    send Value(n: 10)            // sends 10
}
```

### 4. Swap Without Temp (Magic!)

Declarative semantics enable patterns impossible in imperative languages:

```intent
// Swap x and y - no temp variable needed!
effect {
    x = y  // x' = current y
    y = x  // y' = current x
}

// Both reads happen from current state
// Both writes go to next state
// Order doesn't matter:
effect {
    y = x  // Same result!
    x = y
}
```

### 2. Order Independence

If reordering statements changes the meaning, you're thinking imperatively:

```intent
// These two are IDENTICAL semantically:
effect {
    x = 5
    send M(val: x)
}

effect {
    send M(val: x)  // Order doesn't matter!
    x = 5
}
```

Both send the current value of `x` and set `x'` to 5.

### 3. No Intermediate States

You cannot create temporary intermediate states within a transition:

```intent
// ❌ This doesn't work as you might expect:
effect {
    temp = computeValue()
    result = temp * 2     // Reads current temp, not computed value!
}

// ✅ Instead, inline or use a single expression:
effect {
    result = computeValue() * 2
}
```

## Why Declarative Semantics?

### 1. Natural TLA+ Mapping

Intent transpiles to TLA+, which is inherently declarative:

**Intent:**
```intent
effect {
    send M(val: x + 1)
    x = x + 1
}
```

**TLA+:**
```tla
Action ==
    /\ channel' = Append(channel, [val: x + 1])
    /\ x' = x + 1
```

Perfect 1:1 correspondence!

### 2. Atomic Transitions

In concurrent systems, transitions must be atomic. Declarative semantics enforce this:

```intent
// This is ONE atomic step:
effect {
    balance = balance - amount
    send Transfer(amount: amount)
}

// Not two sequential steps that could be interrupted!
```

### 3. Formal Verification

Model checkers (TLC, Apalache) work with declarative state transitions:

- Current state: `{x: 5, y: 10}`
- Action: `x' = y ∧ y' = x`
- Next state: `{x: 10, y: 5}`

No intermediate states to reason about!

### 4. Mathematical Clarity

Declarative effects are mathematical relations:

```
Effect = { (s, s') | P(s, s') }
```

Where `s` is current state and `s'` is next state, related by predicate `P`.

## Comparison Table

| Aspect | Imperative (C/Python/Java) | Declarative (Intent/TLA+/Alloy) |
|--------|---------------------------|----------------------------------|
| **Execution** | Sequential statements | Simultaneous updates |
| **Variable reads** | May see new values | Always current state |
| **Order** | Matters | Irrelevant |
| **State changes** | Multiple intermediate | Single atomic |
| **Mental model** | "Do A, then B" | "State S becomes S'" |
| **Concurrency** | Need locks | Atomic by definition |

## Examples from Intent Codebase

### Example 1: Counter (Bounded Variables)

**File:** `examples/bounded_vars/system.intent`

```intent
effect { counter = counter + 1 }
```

**Generated TLA+:**
```tla
/\ counter' = counter + 1
```

Simple, clean, atomic.

### Example 2: Message Passing (Original)

**File:** `examples/messaging/test.intent`

```intent
effect {
    orderID = "123"
    send PaymentService.PaymentRequested(orderId: orderID, amount: 100)
}
```

**Generated TLA+:**
```tla
/\ orderID' = "123"
/\ PaymentService_queue' = PaymentService_queue \o
     <<[type |-> "PaymentRequested", payload |-> <<orderID, 100>>]>>
```

Note: `orderID` (unprimed) is used in the message, reading current state.

### Example 3: Corrected Message Passing

**File:** `examples/composed/request_response.intent`

```intent
effect {
    send RequestChannel.Query(id: 1)
    requestId = 1
}
```

**Generated TLA+:**
```tla
/\ RequestChannel_queue' = Append(RequestChannel_queue, [type: "Query", arg0: 1])
/\ requestId' = 1
```

Inline value (1) used in message - order of statements irrelevant.

## Debugging Tips

### Problem: Message has wrong value

**Symptom:**
```intent
effect {
    x = 100
    send M(val: x)  // Expected 100, got old value
}
```

**Solution:**
```intent
effect {
    send M(val: 100)  // Use inline value
    x = 100
}
```

### Problem: Swap doesn't work

**Symptom:**
```intent
effect {
    temp = x
    x = y
    y = temp  // Both x and y become y!
}
```

**Solution:**
```intent
effect {
    x = y  // No temp needed
    y = x
}
```

### Problem: Computed value not used

**Symptom:**
```intent
effect {
    result = compute()
    use(result)  // Uses old result, not computed value!
}
```

**Solution:**
```intent
effect {
    use(compute())  // Inline the computation
    result = compute()  // Also store if needed
}
```

Or split into two transitions if you need sequential computation.

## Advanced: When You Need Sequential Execution

If you truly need sequential steps, use multiple transitions:

```intent
// Two atomic steps with intermediate state:
states {
    start
    intermediate
    end
}

transitions {
    start -> intermediate on step1
        effect { x = compute() }

    intermediate -> end on step2
        effect { y = x }  // Now x has the computed value
}
```

This creates an observable intermediate state, which affects:
- What other behaviors can observe
- How the system can be interrupted
- The state space size (for model checking)

## Conclusion

Intent's declarative effect semantics:
- ✅ Enable natural TLA+ transpilation
- ✅ Enforce atomic transitions
- ✅ Support formal verification
- ✅ Provide mathematical clarity
- ✅ Match other specification languages (TLA+, Alloy, Z)

Think of effect blocks as **describing a state transformation**, not **executing a sequence of commands**.

## See Also

- `examples/composed/README.md` - Parallel composition examples
- `LANGUAGE.md` - Intent language reference
- `docs/VERIFICATION_GUIDE.md` - Model checking guide
