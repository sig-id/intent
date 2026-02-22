# Parallel Composition Examples

This directory contains examples demonstrating Intent's parallel composition feature, where multiple behaviors run concurrently and communicate via message passing.

## Examples

### 1. Producer-Consumer (`producer_consumer.intent`)

**Pattern:** One-way message passing

A simple example showing:
- Producer sends messages on a channel
- Consumer receives from the same channel
- Basic asynchronous communication

**Key Concepts:**
- Single message channel
- One sender, one receiver
- Non-blocking send, blocking receive

### 2. Request-Response (`request_response.intent`)

**Pattern:** Bidirectional communication

Demonstrates:
- Client sends request, waits for response
- Server receives request, processes, sends response
- Two separate message channels (request and response)
- Variable usage in behaviors

**Key Concepts:**
- Multiple message channels
- Bidirectional message flow
- State machines synchronized via messages

### 3. Order-Payment (`order_payment.intent`)

**Pattern:** Business workflow with error handling

Shows:
- Order processing workflow
- Payment service interaction
- Multiple possible outcomes (success/failure)
- Realistic message payloads with multiple fields

**Key Concepts:**
- Multiple transitions from same state
- Error handling paths
- Complex message structures

## Understanding Effect Semantics

**IMPORTANT:** Intent uses declarative semantics, not imperative/sequential execution.

### The Key Rule

In an effect block:
- **Variable reads** → see the **current state** (before transition)
- **Variable writes** → create the **next state** (after transition)

### Common Mistake

```intent
// ❌ INCORRECT assumption (imperative thinking):
effect {
    x = 5           // "Set x to 5"
    send M(val: x)  // "Send the new value (5)"
}

// Reality: sends OLD value of x, not 5!
```

### Correct Approach

```intent
// ✅ CORRECT: Use inline values
effect {
    send M(val: 5)  // Send the literal value
    x = 5           // Order doesn't matter!
}

// ✅ CORRECT: Send computed value
effect {
    send M(val: counter + 1)  // Sends current counter + 1
    counter = counter + 1     // Updates counter
}

// ✅ CORRECT: Intentionally use current state
effect {
    send Snapshot(oldValue: x)  // Capture current value
    x = 0                       // Reset x
}
```

### Why This Design?

Intent is a **formal specification language** designed for:
1. **Modeling concurrent systems** - atomic transitions matter
2. **TLA+ generation** - TLA+ actions are declarative
3. **Formal verification** - easier to reason about and model-check

This is the same as TLA+, Alloy, and other formal specification languages.

### Mental Model

Think of an effect block as a mathematical formula describing the relationship between current state and next state:

```
Effect Block = StateTransition(current_state) → next_state
```

All statements execute **simultaneously**, like assignments in mathematics:
```
x' = y   ∧   y' = x     (swap values)
```

Not sequentially like imperative code:
```
temp = x; x = y; y = temp;  // Sequential steps
```

## Compiling Examples

```bash
# Compile all examples
cargo run --release -- compile examples/composed --output /tmp/tla_out

# Check generated TLA+ files
ls /tmp/tla_out/
```

## Generated TLA+ Structure

Each composed system generates:

**Individual behaviors:**
- `{System}_{Behavior1}.tla`
- `{System}_{Behavior2}.tla`

**Parallel composition:**
- `{System}_{ComposedName}.tla`

The composed module contains:
- **Namespaced variables** for each behavior (`Behavior_var`)
- **Shared message queues** (`Channel_queue`)
- **Independent initialization** of each behavior
- **Interleaved transitions** (non-deterministic execution)
- **UNCHANGED clauses** preserving isolation

## Model Checking

Use TLC or Apalache to verify properties:

```bash
# Install TLC
# Download from: https://github.com/tlaplus/tlaplus/releases

# Check the composed system
tlc ProducerConsumer_System.tla

# Or with Apalache (better for larger systems)
apalache-mc check ProducerConsumer_System.tla
```

## Tips

1. **Message payloads**: Use inline values or expressions, not variables you just assigned
2. **Order independence**: Statements in effect blocks can be reordered without changing semantics
3. **Atomicity**: The entire effect block is one atomic transition
4. **Read the generated TLA+**: Understanding the output helps debug unexpected behavior

## See Also

- `docs/PHASE3_PARALLEL_COMPOSITION.md` - Full implementation details
- `LANGUAGE.md` - Intent language reference
- `docs/VERIFICATION_GUIDE.md` - Model checking guide
