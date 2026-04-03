## Intermediate Codegen Test Fixtures

These Verilog files live in `tests/ir_fixtures/` and are meant to drive the
future intermediate representation (IR) generator. The fixtures are isolated
from the core code and do not currently participate in `cargo test` until the
IR layer exists.

### 1. `ir_comb_simple.v`

**Goal**: Basic combinational expressions and operator precedence.

- Tests:
  - Bitwise AND/OR and unary NOT.
  - Parenthesized subexpressions.
- IR expectations:
  - A single combinational block.
  - Correct ordering of operations `(a & b)` then `| ~c`.

### 2. `ir_comb_chain.v`

**Goal**: Arithmetic chain with mixed precedence.

- Expression: `y = a + b * c - 4'd1;`
- IR expectations:
  - `b * c` evaluated before `+` / `-`.
  - Left-associative lowering: `((a) + (b * c)) - 1`.

### 3. `ir_seq_counter.v`

**Goal**: Simple sequential logic with reset.

- Rising-edge + async reset always block.
- Non-blocking assignments.
- IR expectations:
  - One clocked process.
  - Two control-flow paths:
    - Reset path (`q <= 0`).
    - Increment path (`q <= q + 1`).

### 4. `ir_branch_if.v`

**Goal**: If/else branching in a combinational always block.

- IR expectations:
  - Branching structure with two assignments to `y`.
  - Single combinational process, no latches after lowering.

### 5. `ir_branch_case.v`

**Goal**: Case statement lowering to multi-way branch.

- IR expectations:
  - One combinational process with a decision tree on `sel`.
  - Default branch present.

### 6. `ir_memory.v`

**Goal**: Simple register file.

- 4-entry memory `mem[0:3]`.
- Write on `we` at clock edge; combinational read.
- IR expectations:
  - Memory abstraction or lowered per-element registers.
  - Indexed read/write operations surfaced in IR.

### 7. `ir_hierarchy.v`

**Goal**: Small module hierarchy feeding into IR.

- `ir_top` → `ir_mid` → two `ir_leaf` instances.
- IR expectations:
  - Module-level IR that can either be:
    - Kept hierarchical, or
    - Inlined/flattended with proper signal wiring.

### How to use

- For now these fixtures are **input programs only**.
- When the IR generator is implemented, add integration tests that:
  - Build IR from each fixture.
  - Assert on:
    - Number of basic blocks.
    - Operations in each block.
    - Correct control-flow graph and data dependencies.

