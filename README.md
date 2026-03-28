# Bengal

A statically-typed, expression-oriented programming language that compiles to native code via LLVM.

> **Note**: Bengal is under active development. The language specification and compiler internals are subject to breaking changes.

## Features

- **Static typing** with local type inference
- **Expression-oriented** control flow (`if`/`while`/blocks yield values)
- **Generics** with monomorphization and protocol constraints
- **Protocols** for shared interfaces (static dispatch)
- **Structs** with value-type semantics, methods, and computed properties
- **Fixed-size arrays** (stack-allocated)
- **Module system** with visibility control and separate compilation
- **Native code generation** via LLVM

## Example

```bengal
struct Counter {
    var value: Int32;

    init(start: Int32) {
        self.value = start;
    }

    func increment() {
        self.value = self.value + 1;
    }

    func get() -> Int32 {
        return self.value;
    }
}

func fibonacci(n: Int32) -> Int32 {
    var a = 0;
    var b = 1;
    var i = 0;
    while i < n {
        let next = a + b;
        a = b;
        b = next;
        i = i + 1;
    };
    return a;
}

func main() -> Int32 {
    var c = Counter(start: 0);
    let fib = fibonacci(10);

    let result = if fib > 50 {
        yield fib;
    } else {
        c.increment();
        yield c.get();
    };

    return result;
}
```

## Quick Start

### Prerequisites

- Rust toolchain
- LLVM 20.1
- A C linker (`cc`)

### Build

```sh
cargo build --release
```

### Usage

```sh
# Compile a Bengal source file to an executable
bengal compile src/main.bengal

# Compile and print the Bengal IR
bengal compile src/main.bengal --emit-bir

# Evaluate an expression
bengal eval "2 + 3"
```

## Language Overview

### Types

| Type | Description |
|------|-------------|
| `Int32`, `Int64` | Signed integers (literals default to `Int32`) |
| `Float32`, `Float64` | Floating point (literals default to `Float64`) |
| `Bool` | Boolean (`true` / `false`) |
| `()` | Unit type |
| `[T; N]` | Fixed-size array |

### Variables

```bengal
let x = 42;           // immutable (type inferred)
let y: Int32 = 10;    // explicit type annotation
var z = 0;            // mutable
z = z + 1;
```

### Functions

```bengal
func add(a: Int32, b: Int32) -> Int32 {
    return a + b;
}
```

### Generics

```bengal
protocol Summable {
    func sum() -> Int32;
}

func total<T: Summable>(a: T, b: T) -> Int32 {
    return a.sum() + b.sum();
}
```

### Control Flow

```bengal
// if/else as expressions
let sign = if x > 0 {
    yield 1;
} else {
    yield 0;
};

// while with break values
let found = while i < n {
    if arr[i] == target {
        break i;
    };
    i = i + 1;
} nobreak {
    yield 0;
};
```

### Modules

```bengal
// math.bengal
module math;
public func square(x: Int32) -> Int32 {
    return x * x;
}

// main.bengal
import math::square;
func main() -> Int32 {
    return square(5);
}
```

### Visibility

Five levels: `public`, `package`, `internal` (default), `fileprivate`, `private`.

## Compilation Pipeline

```
parse → analyze → lower → optimize → monomorphize → codegen → link
```

| Stage | Description |
|-------|-------------|
| **parse** | Tokenization (logos) and AST construction |
| **analyze** | Name resolution, type inference, type checking |
| **lower** | Convert AST to Bengal IR (BIR) |
| **optimize** | Constant folding, dead code elimination |
| **monomorphize** | Specialize generic functions and structs |
| **codegen** | Generate LLVM IR (inkwell) |
| **link** | Link object files via system C compiler |

## License

[MIT](LICENSE)
