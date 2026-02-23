# Intent Effect Blocks - Quick Reference

## Key Principle

Effect blocks are **DECLARATIVE** (like TLA+), not imperative.

**All statements happen SIMULTANEOUSLY** in a single atomic transition.

## Variable Reference Rules

| Code | Meaning | Reads From | Writes To |
|------|---------|------------|-----------|
| `x` | Variable read | Current state | - |
| `x = expr` | Assignment | - | Next state |
| `expr` | Expression | Current state | - |

## THE Key Insight

**STATEMENT ORDER IS IRRELEVANT** - All updates happen simultaneously!

```intent
// These are IDENTICAL:
effect {               effect {
    x = 5                  send M(val: x)
    send M(val: x)         x = 5
}                      }
```

Both send current x, both set x' to 5.

## Understanding Variable Reads

Variable reads **ALWAYS** see current state (never next state):

```intent
// If counter = 5:
effect {
    counter = counter + 1        // counter' = 6
    send Value(n: counter)       // sends 5 (NOT 6!)

    // The read of counter sees current state (5)
    // The write of counter goes to next state (6)
    // Order of these lines doesn't matter!
}
```

## Common Patterns

### ✅ Understanding The Model

```intent
// Order irrelevant - both send current x
effect {
    send Snapshot(val: x)
    x = x + 1
}

// Swap without temp - simultaneous updates!
effect {
    x = y  // x' = current y
    y = x  // y' = current x
}

// Send computed value from current state
effect {
    send Delta(d: counter + 1)  // computes from current
    counter = counter + 1       // writes to next
}

// Send literal, assign same literal
effect {
    send Request(id: 123)
    requestId = 123
}
```

### ❌ Common Misconceptions

```intent
// ❌ Thinking order matters
effect {
    x = 5
    send M(val: x)  // "I set x first, so it should send 5"
}
// Reality: Sends current x. Order doesn't change this!

// ❌ Thinking temp holds new value
effect {
    temp = compute()
    y = temp  // "temp should have computed value"
}
// Reality: ALL reads see current state

// ❌ Thinking you can build up state
effect {
    a = f()
    b = g(a)  // "b should use new a"
    c = h(b)  // "c should use new b"
}
// Reality: All reads see current state, not new values
```

## Mental Model

Think **DECLARATIVELY** (math/logic):
- "What is the next state?"
- "All changes happen at once"
- Order is **irrelevant**

Not **IMPERATIVELY** (programming):
- "Do this, then do that"
- "Changes happen step by step"
- Order matters

## Quick Checklist

- [ ] Messages use inline values or expressions (not just-assigned variables)
- [ ] Order of statements doesn't affect meaning
- [ ] No assumption that one statement "sees" another's effect
- [ ] Think: "all updates simultaneous" not "execute in sequence"

## When You Need Sequential

Use multiple transitions with intermediate states:

```intent
states { s1, s2, s3 }

transitions {
    s1 -> s2 on step1
        effect { x = compute() }

    s2 -> s3 on step2
        effect { y = x }  // Now x has computed value
}
```

## See Full Documentation

- `docs/EFFECT_SEMANTICS.md` - Complete explanation
- `examples/composed/README.md` - Parallel composition examples
- `LANGUAGE.md` - Language reference
