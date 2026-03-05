# TLA+ Verification Guide

The Intent compiler now includes comprehensive TLA+ verification capabilities using both **Apalache** (symbolic model checker) and **TLC** (exhaustive model checker).

## Quick Start

```bash
# Compile Intent specifications to TLA+
intent compile examples/minimal --output tmp/minimal

# Fast verification with Apalache (bounded model checking)
intent verify --obligations tmp/minimal --mode fast

# Exhaustive verification with TLC (complete state space)
intent verify --obligations tmp/minimal --mode exhaustive --temporal

# Both Apalache and TLC
intent verify --obligations tmp/minimal --mode both --temporal
```

## Verification Modes

### Fast Mode (Apalache)
Uses symbolic model checking for **fast, bounded verification**:
- ✓ Type checking
- ✓ State invariants (TypeOK, HistoryConsistent, custom invariants)
- ✓ Bounded safety properties (up to N steps)
- ⚠️ Temporal properties not fully supported

**When to use**: Quick iteration during development, CI/CD pipelines

```bash
intent verify --obligations tmp/minimal --mode fast --length 20
```

### Exhaustive Mode (TLC)
Uses complete state space exploration:
- ✓ All state invariants
- ✓ Temporal properties (LTL)
- ✓ Liveness properties
- ✓ Fairness constraints
- ✓ Complete verification (not bounded)

**When to use**: Final verification, production deployments, temporal properties

```bash
intent verify --obligations tmp/minimal --mode exhaustive --temporal
```

### Both Mode
Runs both verifiers for comprehensive verification:
- Apalache for type checking and fast safety checks
- TLC for exhaustive exploration and temporal properties

```bash
intent verify --obligations tmp/minimal --mode both --temporal
```

## What Gets Verified

### 1. Type Checking (Apalache)
Ensures all expressions have consistent types:
```tla
TypeOK ==
    /\ state \in States
    /\ pc \in Nat
```

### 2. State Invariants
Properties that must hold in every reachable state:
- `TypeOK`: Type correctness
- `HistoryConsistent`: Bookkeeping invariants
- Custom invariants from `invariant` blocks

### 3. Temporal Properties
Properties about execution paths (requires TLC):
```tla
Prop_eventual_completion ==
    []((state = created) => (<>((state = completed) \/ (state = cancelled))))
```

### 4. Liveness Properties
Eventually-properties:
```tla
Liveness == <>(state \in {completed, cancelled})
```

### 5. Fairness Constraints
Ensure progress under fairness assumptions:
```tla
Fairness_created_to_assigned == SF_vars(created_assign)
```

## Command Options

```bash
intent verify [OPTIONS] --obligations <DIRECTORY>

Options:
  --obligations <DIR>      Directory containing TLA+ files
  --mode <MODE>            Verification mode [fast|exhaustive|both]
  --length <N>             Max steps for bounded checking (default: 10)
  --temporal               Check temporal properties (requires TLC)
  --format <FORMAT>        Output format [text|json]
  --quiet                  Suppress output
```

## Output Format

### Text Format (default)
```
  [PASS] TaskManager_TaskLifecycle (14.61s)
    [✓] Type checking
    [✓] TypeOK (14 states)
    [✓] HistoryConsistent
    [✓] Liveness (tlc)
    [✓] Prop_eventual_completion (tlc)

Success: 2/2 modules verified
```

### JSON Format
```bash
intent verify --obligations tmp/minimal --mode fast --format json
```

```json
{
  "module": "TaskManager_TaskLifecycle",
  "file": "tmp/minimal/TaskManager_TaskLifecycle.tla",
  "type_check": {
    "name": "TypeCheck",
    "passed": true,
    "checker": "apalache"
  },
  "invariants": [
    {
      "name": "TypeOK",
      "passed": true,
      "checker": "apalache",
      "states_checked": 14
    }
  ],
  "status": "Pass",
  "duration": 14.61
}
```

## TLC Configuration Files

For exhaustive verification, TLC uses `.cfg` files. The compiler can generate these automatically:

```intent
behavior TaskLifecycle {
    // ... behavior definition ...
}
```

Generated `.cfg` file:
```tla
SPECIFICATION Spec
INVARIANTS
  TypeOK
  HistoryConsistent
PROPERTIES
  Liveness
  Prop_eventual_completion
```

Create manually:
```bash
cat > tmp/minimal/TaskManager_TaskLifecycle.cfg <<'EOF'
SPECIFICATION Spec
INVARIANTS TypeOK
PROPERTIES Liveness
EOF
```

## Example Workflows

### Development Cycle
```bash
# 1. Compile to TLA+
intent compile examples/payment --output tmp/payment

# 2. Quick verification during development
intent verify --obligations tmp/payment --mode fast

# 3. Full verification before commit
intent verify --obligations tmp/payment --mode exhaustive --temporal
```

### CI/CD Pipeline
```yaml
# .github/workflows/verify.yml
- name: Compile TLA+ specifications
  run: intent compile intent/ --output formal/tla/

- name: Fast verification (Apalache)
  run: intent verify --obligations formal/tla/ --mode fast --length 20

- name: Exhaustive verification (TLC)
  run: intent verify --obligations formal/tla/ --mode exhaustive --temporal
  if: github.ref == 'refs/heads/main'  # Only on main branch
```

### Production Deployment
```bash
# Full verification with both checkers
intent verify --obligations formal/tla/ --mode both --temporal --length 50
```

## Performance Considerations

### Apalache (Fast Mode)
- **Speed**: Seconds to minutes
- **State space**: Bounded by `--length`
- **Memory**: Efficient (symbolic)
- **Best for**: Quick iteration, large state spaces

### TLC (Exhaustive Mode)
- **Speed**: Seconds to hours (depends on state space)
- **State space**: Complete exploration
- **Memory**: Stores all states
- **Best for**: Final verification, small to medium state spaces

### Optimization Tips

1. **Use fast mode during development**:
   ```bash
   intent verify --obligations tmp/minimal --mode fast --length 10
   ```

2. **Increase length for deeper checking**:
   ```bash
   intent verify --obligations tmp/minimal --mode fast --length 50
   ```

3. **Use TLC for final verification**:
   ```bash
   intent verify --obligations tmp/minimal --mode exhaustive --temporal
   ```

4. **For distributed systems**, provide constant values:
   ```bash
   # Create .cfg file with constants
   cat > module.cfg <<'EOF'
   CONSTANTS replicas = {"n1", "n2", "n3"}
   EOF
   ```

## Troubleshooting

### "Too many arguments" error
Update to latest Apalache version. Use `=` for options:
```bash
apalache-mc check --inv=TypeOK --length=10 module.tla
```

### "Directory already exists" (TLC)
TLC creates timestamped directories. The tool handles this automatically, but you can clean up:
```bash
rm -rf .tlc_work_*
```

### Distributed system verification fails
Distributed behaviors need constant assignments:
```tla
CONSTANTS replicas = {"n1", "n2", "n3"}
```

Create a `.cfg` file or the spec will fail type checking.

### Timeout during verification
For large state spaces:
1. Reduce `--length` for Apalache
2. Use symmetry in TLC `.cfg`:
   ```tla
   SYMMETRY Permutations
   ```
3. Consider abstracting the model

## Verification Results

### All Test Examples Pass! ✓

```
Minimal Examples:
  ✓ TaskManager_TaskLifecycle      (Apalache + TLC)
  ✓ TaskManager_UserRegistration   (Apalache + TLC)

Payment Examples:
  ✓ PaymentPlatform_TransactionLifecycle  (Apalache + TLC)
  ✓ PaymentPlatform_SettlementSaga        (Apalache + TLC)

Distributed Examples:
  ✓ DistributedCache_FailureDetection   (Apalache)
  ✓ DistributedCache_Failover            (Apalache)
  ✓ DistributedCache_WriteReplication    (Apalache)
  ✓ DistributedCache_NodeLifecycle       (TLC with constants)
```

## Further Reading

- [TLA+ Homepage](https://lamport.azurewebsites.net/tla/tla.html)
- [Apalache Documentation](https://apalache.informal.systems/)
- [TLC Documentation](https://lamport.azurewebsites.net/tla/tools.html)
- [Learn TLA+](https://learntla.com/)
