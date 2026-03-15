<!-- status: FUTURE -->
# Multi-Language Support Implementation Plans

> **For agentic workers:** Each language section is an independent plan. Dispatch one fleet agent per language. All follow the same 7-subsystem template. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring all target languages to full APEX support: enum → runner → instrumentor → sandbox → index → detectors → call graph.

**Architecture:** Each language implements the same 7 traits/interfaces. New languages need all 7; partially-supported languages only need the gaps filled. Every subsystem follows existing patterns — copy the closest existing language implementation and adapt.

**Tech Stack:** Rust, async-trait, regex, serde, per-language coverage tools

---

## Subsystem Template

Every language needs these 7 components:

| # | Subsystem | Crate | Trait/Pattern | Reference Implementation |
|---|-----------|-------|--------------|-------------------------|
| 1 | Language enum | `apex-core/src/types.rs` | `Language` enum + `FromStr` | Python variant |
| 2 | Runner | `apex-lang/src/<lang>.rs` | `LanguageRunner` trait | `python.rs` |
| 3 | Instrumentor | `apex-instrument/src/<lang>.rs` | `Instrumentor` trait | `python.rs` |
| 4 | Sandbox | `apex-sandbox/src/<lang>.rs` | `Sandbox` trait | `python.rs` |
| 5 | Index | `apex-index/src/<lang>.rs` | Coverage parsing + `BranchIndex` | `python.rs` |
| 6 | Detectors | `apex-detect/src/detectors/security_pattern.rs` | `SecurityPattern` array | Python patterns |
| 7 | Call graph | `apex-reach/src/extractors/<lang>.rs` | `CallGraphExtractor` trait | `python.rs` |

Plus CLI integration: wire the new language into `apex-cli/src/lib.rs::instrument()`.

---

## Plan A: JavaScript/TypeScript — Close Gaps

**Current state:** 90% complete. Missing index support only.

**Files:**
- Create: `crates/apex-index/src/javascript.rs`
- Modify: `crates/apex-index/src/lib.rs` (add module)

### Task A1: JS/TS index support

Parse V8/c8/nyc coverage JSON → `BranchIndex`. Follow `python.rs` pattern in apex-index.

- [ ] **Step 1:** Read `crates/apex-index/src/python.rs` to understand the index pattern
- [ ] **Step 2:** Create `crates/apex-index/src/javascript.rs` — parse c8/v8 JSON coverage output, build `TestTrace` entries, populate `BranchIndex`
- [ ] **Step 3:** Coverage JSON format: `{"result": [{"url": "file:///...", "functions": [{"functionName": "...", "ranges": [...]}]}]}` (V8 format) or Istanbul JSON `{"path": {"statementMap": {...}, "s": {...}}}` (nyc/c8)
- [ ] **Step 4:** Support both V8 and Istanbul formats (detect by structure)
- [ ] **Step 5:** Wire into `apex-index/src/lib.rs` module list
- [ ] **Step 6:** Tests: parse fixture JSON, verify branch count, verify test-to-branch mapping
- [ ] **Step 7:** Commit

**Effort:** 1 task, ~200 lines

---

## Plan B: Java + Kotlin

**Current state:** Java 80% (missing index, call graph). Kotlin 0% (shares Java infra).

**Files:**
- Create: `crates/apex-index/src/java.rs`
- Create: `crates/apex-reach/src/extractors/java.rs`
- Modify: `crates/apex-core/src/types.rs` (add `Kotlin` variant)
- Create: `crates/apex-lang/src/kotlin.rs`
- Modify: `crates/apex-reach/src/extractors/mod.rs` (add Java + Kotlin)

### Task B1: Java index support

Parse JaCoCo XML/CSV coverage → `BranchIndex`.

- [ ] **Step 1:** Create `crates/apex-index/src/java.rs`
- [ ] **Step 2:** Parse JaCoCo XML: `<report><package><class><method><counter type="BRANCH" missed="X" covered="Y"/>`
- [ ] **Step 3:** Map JaCoCo counters to `BranchId` (file_id from class path, line from `<line>` elements)
- [ ] **Step 4:** Tests with JaCoCo XML fixture
- [ ] **Step 5:** Commit

### Task B2: Java call graph extractor

- [ ] **Step 1:** Create `crates/apex-reach/src/extractors/java.rs`
- [ ] **Step 2:** Function detection: `(public|private|protected)?\s*(static\s+)?\w+\s+(\w+)\s*\(` — method signatures
- [ ] **Step 3:** Call detection: `(\w+)\s*\(`, `this\.(\w+)\s*\(`, `(\w+)\.(\w+)\s*\(`
- [ ] **Step 4:** Entry points:
  - `@Test` / `@ParameterizedTest` → Test
  - `public static void main` → Main
  - `@GetMapping` / `@PostMapping` / `@RequestMapping` → HttpHandler
  - `public` methods in `@RestController` / `@Service` → PublicApi
  - `@SpringBootApplication` → CliEntry
- [ ] **Step 5:** Register in `extractors/mod.rs` dispatch
- [ ] **Step 6:** Tests, commit

### Task B3: Kotlin language support

Kotlin shares Java's build tools (Gradle/Maven), coverage (JaCoCo), and security patterns. Add as a separate enum variant that reuses Java infrastructure.

- [ ] **Step 1:** Add `Kotlin` to `Language` enum in `apex-core/src/types.rs`
- [ ] **Step 2:** Add `FromStr`: `"kotlin" | "kt"` → `Language::Kotlin`
- [ ] **Step 3:** Create `crates/apex-lang/src/kotlin.rs` — `KotlinRunner`:
  - `detect()`: `build.gradle.kts` or `*.kt` files present
  - `install_deps()`: delegate to Java's Gradle/Maven
  - `run_tests()`: `./gradlew test` (same as Java)
- [ ] **Step 4:** Wire into `apex-cli/src/lib.rs::instrument()` — map Kotlin → Java instrumentor
- [ ] **Step 5:** Add Kotlin security patterns to `security_pattern.rs`:
  - `Runtime.getRuntime().exec(` (CWE-78)
  - `ProcessBuilder(` (CWE-78)
  - `statement.executeQuery(` + string interpolation (CWE-89)
  - `ObjectMapper().readValue(` (CWE-502)
  - `URL(userInput)` (CWE-918)
- [ ] **Step 6:** Add Kotlin call graph extractor (shares Java's — `fun` instead of return-type method signature)
- [ ] **Step 7:** Tests, commit

**Effort:** 3 tasks, ~600 lines

---

## Plan C: Go

**Current state:** 0%. New language from scratch.

**Files:**
- Modify: `crates/apex-core/src/types.rs` (add `Go` variant)
- Create: `crates/apex-lang/src/go.rs`
- Create: `crates/apex-instrument/src/go.rs`
- Create: `crates/apex-index/src/go.rs`
- Create: `crates/apex-reach/src/extractors/go.rs`
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs` (add Go patterns)
- Modify: `crates/apex-cli/src/lib.rs` (wire Go)

### Task C1: Go language enum + runner

- [ ] **Step 1:** Add `Go` to `Language` enum, `FromStr`: `"go" | "golang"` → `Language::Go`
- [ ] **Step 2:** Create `crates/apex-lang/src/go.rs` — `GoRunner`:
  - `detect()`: `go.mod` or `go.sum` exists
  - `install_deps()`: `go mod download`
  - `run_tests()`: `go test ./...` with `-v` flag
- [ ] **Step 3:** Tests: detect go.mod, mock runner for install/test
- [ ] **Step 4:** Commit

### Task C2: Go instrumentor

- [ ] **Step 1:** Create `crates/apex-instrument/src/go.rs` — `GoInstrumentor`:
  - Run: `go test -coverprofile=coverage.out -covermode=atomic ./...`
  - Parse `coverage.out` format: `mode: atomic` header, then `file:startLine.startCol,endLine.endCol numStmt count` per line
  - Convert to `BranchId` entries (file_id via fnv1a_hash, line from coverage data)
- [ ] **Step 2:** Handle `go test -json -coverprofile` for structured output
- [ ] **Step 3:** Tests with fixture coverage.out file
- [ ] **Step 4:** Commit

### Task C3: Go index support

- [ ] **Step 1:** Create `crates/apex-index/src/go.rs`
- [ ] **Step 2:** Parse `go test -json` output to extract per-test results + coverage
- [ ] **Step 3:** Build `TestTrace` from `go test -coverprofile` per-test runs
- [ ] **Step 4:** Tests, commit

### Task C4: Go security detectors

- [ ] **Step 1:** Add Go patterns to `security_pattern.rs`:
  - `exec.Command(` (CWE-78) — command injection
  - `sql.Open(` / `db.Query(` + string concat (CWE-89) — SQL injection
  - `http.Get(variable)` / `http.Post(variable)` (CWE-918) — SSRF
  - `template.HTML(` (CWE-79) — XSS via unescaped HTML
  - `os.Open(variable)` / `os.ReadFile(variable)` (CWE-22) — path traversal
  - `json.Unmarshal(` into `interface{}` (CWE-502) — unsafe deserialization
  - `md5.New()` / `sha1.New()` (CWE-327) — weak crypto
  - `fmt.Sprintf` in SQL context (CWE-89) — format string SQL
  - `log.Fatal(` / `os.Exit(` in library (CWE-705) — exit in library
- [ ] **Step 2:** Add Go-specific detectors if patterns need multi-line analysis
- [ ] **Step 3:** Tests, commit

### Task C5: Go call graph extractor

- [ ] **Step 1:** Create `crates/apex-reach/src/extractors/go.rs`
- [ ] **Step 2:** Function detection: `func\s+(\w+)\s*\(` and `func\s+\(\w+\s+\*?\w+\)\s+(\w+)\s*\(` (methods)
- [ ] **Step 3:** Call detection: `(\w+)\s*\(`, `(\w+)\.(\w+)\s*\(`
- [ ] **Step 4:** Entry points:
  - `func Test\w+(t *testing.T)` → Test
  - `func Benchmark\w+(b *testing.B)` → Test
  - `func main()` → Main
  - `http.HandleFunc(` / `mux.HandleFunc(` / `r.GET(` (gin) → HttpHandler
  - Exported functions (capitalized) → PublicApi
  - `flag.Parse()` / `cobra.Command` → CliEntry
- [ ] **Step 5:** Register in `extractors/mod.rs`
- [ ] **Step 6:** Tests, commit

### Task C6: Wire Go into CLI

- [ ] **Step 1:** Add `Language::Go` arm in `apex-cli/src/lib.rs::instrument()` → use `GoInstrumentor`
- [ ] **Step 2:** Add Go extensions to `build_source_cache`: `&["go"]`
- [ ] **Step 3:** Verify `cargo check --workspace`
- [ ] **Step 4:** Commit

**Effort:** 6 tasks, ~1200 lines

---

## Plan D: C/C++

**Current state:** C 30% (enum, runner, 5 security patterns). C++ 0%.

**Files:**
- Modify: `crates/apex-core/src/types.rs` (add `Cpp` variant)
- Create: `crates/apex-lang/src/cpp.rs`
- Create: `crates/apex-instrument/src/c_coverage.rs` (shared C/C++)
- Create: `crates/apex-index/src/c_cpp.rs`
- Create: `crates/apex-reach/src/extractors/c_cpp.rs`
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs` (expand C, add C++)

### Task D1: C++ language enum + runner

- [ ] **Step 1:** Add `Cpp` to `Language` enum, `FromStr`: `"cpp" | "c++" | "cxx"` → `Language::Cpp`
- [ ] **Step 2:** Create `crates/apex-lang/src/cpp.rs` — `CppRunner`:
  - `detect()`: `CMakeLists.txt` or `Makefile` with `.cpp`/`.cxx`/`.cc` files
  - `install_deps()`: `cmake -B build` or `make` (detect build system)
  - `run_tests()`: `ctest --test-dir build` or `make test` or detect GoogleTest/Catch2
- [ ] **Step 3:** Tests, commit

### Task D2: C/C++ instrumentor (shared)

- [ ] **Step 1:** Create `crates/apex-instrument/src/c_coverage.rs` — `CCoverageInstrumentor`:
  - **gcov path:** Compile with `-fprofile-arcs -ftest-coverage`, run tests, parse `.gcov` files
  - **llvm-cov path:** Compile with `-fprofile-instr-generate -fcoverage-mapping`, run, `llvm-profdata merge`, `llvm-cov export --format=text`
  - Auto-detect: if `clang` available → llvm-cov, else → gcov
- [ ] **Step 2:** Parse gcov format: line-by-line execution counts (`count:line:source`)
- [ ] **Step 3:** Parse llvm-cov JSON export: `{"data": [{"files": [{"filename": "...", "segments": [...]}]}]}`
- [ ] **Step 4:** Convert to `BranchId` entries
- [ ] **Step 5:** Use for both `Language::C` and `Language::Cpp`
- [ ] **Step 6:** Tests with fixture coverage files
- [ ] **Step 7:** Commit

### Task D3: C/C++ security patterns

- [ ] **Step 1:** Expand C patterns in `security_pattern.rs`:
  - `gets(` (CWE-242) — banned function
  - `strcpy(` / `strcat(` / `sprintf(` (CWE-120) — buffer overflow
  - `printf(variable)` without format string (CWE-134) — format string vuln
  - `malloc(` without bounds check (CWE-190) — integer overflow
  - `free(` followed by use (CWE-416) — use-after-free heuristic
  - `system(` (CWE-78) — command injection
- [ ] **Step 2:** Add C++ patterns:
  - `new` without `delete` / smart pointer (CWE-401) — memory leak heuristic
  - `reinterpret_cast<` (CWE-704) — unsafe cast
  - `std::system(` (CWE-78)
  - `sprintf` / `vsprintf` (CWE-120) — use `snprintf`
  - `std::stoi(` / `atoi(` without try-catch (CWE-754)
  - SQL string concat patterns with libpq/mysql/sqlite3
- [ ] **Step 3:** Tests, commit

### Task D4: C/C++ call graph extractor

- [ ] **Step 1:** Create `crates/apex-reach/src/extractors/c_cpp.rs` — shared extractor
- [ ] **Step 2:** Function detection: C `(\w+)\s+(\w+)\s*\(` , C++ `(\w+)::(\w+)\s*\(` (methods)
- [ ] **Step 3:** Entry points:
  - `int main(` → Main
  - `TEST(` / `TEST_F(` (GoogleTest) → Test
  - `BOOST_AUTO_TEST_CASE(` → Test
  - `TEST_CASE(` (Catch2) → Test
- [ ] **Step 4:** Tests, commit

### Task D5: C/C++ index + CLI wiring

- [ ] **Step 1:** Create `crates/apex-index/src/c_cpp.rs` — parse gcov/llvm-cov output
- [ ] **Step 2:** Wire C++ into `apex-cli/src/lib.rs::instrument()`
- [ ] **Step 3:** Add `["c", "h", "cpp", "cxx", "cc", "hpp", "hxx"]` to `build_source_cache`
- [ ] **Step 4:** Tests, commit

**Effort:** 5 tasks, ~1000 lines

---

## Plan E: Ruby

**Current state:** 10% (enum + 9 security patterns + 5 detectors). No runner/instrumentor/sandbox/index/call graph.

**Files:**
- Create: `crates/apex-lang/src/ruby.rs`
- Create: `crates/apex-instrument/src/ruby.rs`
- Create: `crates/apex-index/src/ruby.rs`
- Create: `crates/apex-reach/src/extractors/ruby.rs`
- Modify: `crates/apex-cli/src/lib.rs` (replace Ruby stub with real implementation)

### Task E1: Ruby runner

- [ ] **Step 1:** Create `crates/apex-lang/src/ruby.rs` — `RubyRunner`:
  - `detect()`: `Gemfile` or `Rakefile` or `*.gemspec` exists
  - `install_deps()`: `bundle install` (Bundler) or `gem install` fallback
  - `run_tests()`: detect RSpec (`spec/` dir) → `bundle exec rspec`, else Minitest → `ruby -Ilib -Itest -e 'Dir.glob("test/**/test_*.rb").each{|f| require f}'`
- [ ] **Step 2:** Tests, commit

### Task E2: Ruby instrumentor

- [ ] **Step 1:** Create `crates/apex-instrument/src/ruby.rs` — `RubyInstrumentor`:
  - Use SimpleCov with JSON formatter: inject `require 'simplecov'; SimpleCov.start` via `RUBYOPT` env
  - Parse SimpleCov JSON: `{"coverage": {"file.rb": {"lines": [null, 1, 0, 2, ...]}}}`
  - Convert line coverage to `BranchId` entries
- [ ] **Step 2:** Handle SimpleCov not installed: `gem install simplecov simplecov-json`
- [ ] **Step 3:** Tests with fixture JSON
- [ ] **Step 4:** Commit

### Task E3: Ruby call graph extractor

- [ ] **Step 1:** Create `crates/apex-reach/src/extractors/ruby.rs`
- [ ] **Step 2:** Function detection: `def\s+(\w+)` (methods), `def\s+self\.(\w+)` (class methods)
- [ ] **Step 3:** Entry points:
  - `describe` / `it` / `context` (RSpec) → Test
  - `def test_\w+` (Minitest) → Test
  - `Rails.application` / `class.*Controller < ApplicationController` → HttpHandler
  - `get '/'` / `post '/'` (Sinatra routes) → HttpHandler
  - `OptionParser.new` → CliEntry
- [ ] **Step 4:** Tests, commit

### Task E4: Ruby index + CLI wiring

- [ ] **Step 1:** Create `crates/apex-index/src/ruby.rs`
- [ ] **Step 2:** Replace Ruby stub in `apex-cli/src/lib.rs::instrument()` with real `RubyInstrumentor`
- [ ] **Step 3:** Tests, commit

**Effort:** 4 tasks, ~800 lines

---

## Plan F: Swift

**Current state:** 0%. New language from scratch.

**Files:**
- Modify: `crates/apex-core/src/types.rs` (add `Swift` variant)
- Create: `crates/apex-lang/src/swift.rs`
- Create: `crates/apex-instrument/src/swift.rs`
- Create: `crates/apex-index/src/swift.rs`
- Create: `crates/apex-reach/src/extractors/swift.rs`
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

### Task F1: Swift language enum + runner

- [ ] **Step 1:** Add `Swift` to `Language` enum, `FromStr`: `"swift"` → `Language::Swift`
- [ ] **Step 2:** Create `crates/apex-lang/src/swift.rs` — `SwiftRunner`:
  - `detect()`: `Package.swift` or `*.xcodeproj` or `*.xcworkspace`
  - `install_deps()`: `swift package resolve` (SPM) or `pod install` (CocoaPods)
  - `run_tests()`: `swift test` (SPM) or `xcodebuild test` (Xcode)
- [ ] **Step 3:** Tests, commit

### Task F2: Swift instrumentor

- [ ] **Step 1:** Create `crates/apex-instrument/src/swift.rs` — `SwiftInstrumentor`:
  - Run: `swift test --enable-code-coverage`
  - Extract profdata: `swift test --show-codecov-path` → get `.json` path
  - Parse: `llvm-cov export` JSON format (same as Rust LLVM coverage)
  - Reuse llvm-cov JSON parsing from `rust_cov.rs` where possible
- [ ] **Step 2:** Tests, commit

### Task F3: Swift security patterns + detectors

- [ ] **Step 1:** Add Swift patterns to `security_pattern.rs`:
  - `Process()` / `NSTask` (CWE-78) — command execution
  - `URLSession.shared.dataTask(with: URL(string: variable)` (CWE-918) — SSRF
  - `NSAppleScript(source:` (CWE-94) — code injection
  - `SecRandomCopyBytes` absence → `arc4random` (CWE-330) — weak random
  - `UserDefaults` for sensitive data (CWE-312) — cleartext storage
  - `URLSession` without certificate pinning (CWE-295) — improper cert validation
  - `NSKeyedUnarchiver.unarchiveObject(` (CWE-502) — insecure deserialization
  - `try!` / `fatalError(` in library code (CWE-705)
- [ ] **Step 2:** Tests, commit

### Task F4: Swift call graph + index + CLI wiring

- [ ] **Step 1:** Create `crates/apex-reach/src/extractors/swift.rs`:
  - Function detection: `func\s+(\w+)\s*\(`, `class\s+(\w+)`, `struct\s+(\w+)`
  - Entry points: `func test\w+()` with `XCTestCase` → Test, `@main` → Main, `UIApplicationDelegate` → Main
- [ ] **Step 2:** Create `crates/apex-index/src/swift.rs`
- [ ] **Step 3:** Wire into CLI
- [ ] **Step 4:** Tests, commit

**Effort:** 4 tasks, ~800 lines

---

## Plan G: C# / .NET

**Current state:** 0%. New language from scratch.

**Files:**
- Modify: `crates/apex-core/src/types.rs` (add `CSharp` variant)
- Create: `crates/apex-lang/src/csharp.rs`
- Create: `crates/apex-instrument/src/csharp.rs`
- Create: `crates/apex-index/src/csharp.rs`
- Create: `crates/apex-reach/src/extractors/csharp.rs`
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

### Task G1: C# language enum + runner

- [ ] **Step 1:** Add `CSharp` to `Language` enum, `FromStr`: `"csharp" | "c#" | "cs" | "dotnet"` → `Language::CSharp`
- [ ] **Step 2:** Create `crates/apex-lang/src/csharp.rs` — `CSharpRunner`:
  - `detect()`: `*.csproj` or `*.sln` or `*.fsproj` exists
  - `install_deps()`: `dotnet restore`
  - `run_tests()`: `dotnet test`
- [ ] **Step 3:** Tests, commit

### Task G2: C# instrumentor

- [ ] **Step 1:** Create `crates/apex-instrument/src/csharp.rs` — `CSharpInstrumentor`:
  - Run: `dotnet test --collect:"XPlat Code Coverage"` (uses Coverlet)
  - Parse Cobertura XML: `<coverage><packages><package><classes><class><lines><line number="N" hits="H"/>`
  - Convert to `BranchId` entries
- [ ] **Step 2:** Alternative: parse Coverlet JSON format
- [ ] **Step 3:** Tests, commit

### Task G3: C# security patterns + detectors

- [ ] **Step 1:** Add C# patterns to `security_pattern.rs`:
  - `Process.Start(` (CWE-78) — command injection
  - `SqlCommand(` + string concat / interpolation (CWE-89) — SQL injection
  - `HttpClient.GetAsync(variable)` (CWE-918) — SSRF
  - `BinaryFormatter.Deserialize(` / `XmlSerializer(` (CWE-502) — insecure deserialization
  - `MD5.Create()` / `SHA1.Create()` (CWE-327) — weak crypto
  - `Response.Write(` (CWE-79) — XSS
  - `File.ReadAllText(variable)` (CWE-22) — path traversal
  - `Environment.Exit(` in library (CWE-705)
  - `dynamic` keyword with external data (CWE-94) — code injection
  - `[AllowAnonymous]` without justification (CWE-862) — missing auth
- [ ] **Step 2:** Tests, commit

### Task G4: C# call graph + index + CLI wiring

- [ ] **Step 1:** Create `crates/apex-reach/src/extractors/csharp.rs`:
  - Function detection: `(public|private|protected|internal)\s+(static\s+)?\w+\s+(\w+)\s*\(`
  - Entry points: `[Test]` / `[Fact]` / `[Theory]` → Test, `static void Main` → Main, `[HttpGet]` / `[HttpPost]` → HttpHandler, `public` in `Controller` class → PublicApi
- [ ] **Step 2:** Create `crates/apex-index/src/csharp.rs`
- [ ] **Step 3:** Wire into CLI
- [ ] **Step 4:** Tests, commit

**Effort:** 4 tasks, ~800 lines

---

## Dispatch Strategy

All 7 plans are independent — dispatch in parallel:

```
Parallel (all independent):
  ├── Plan A: JS/TS gaps        (1 task)   → intelligence crew
  ├── Plan B: Java + Kotlin      (3 tasks)  → intelligence crew
  ├── Plan C: Go                 (6 tasks)  → intelligence crew
  ├── Plan D: C/C++              (5 tasks)  → intelligence + runtime crew
  ├── Plan E: Ruby               (4 tasks)  → runtime crew
  ├── Plan F: Swift              (4 tasks)  → intelligence crew
  └── Plan G: C# / .NET          (4 tasks)  → intelligence crew
```

Within each plan, tasks are sequential (each builds on the previous).

**Crew assignments:**
- **Intelligence crew** owns `apex-reach` extractors
- **Runtime crew** owns `apex-lang`, `apex-instrument`, `apex-sandbox`, `apex-index`
- **Security-detect crew** owns `apex-detect` patterns
- **Platform crew** owns `apex-cli` wiring

For each language, dispatch the runtime crew first (runner + instrumentor), then intelligence (call graph), then security (detectors), then platform (CLI wiring). Or dispatch a single agent per language that handles all subsystems.

---

## Projected Result

| Language | Before | After |
|----------|--------|-------|
| Python | Full | Full |
| Rust | Full | Full |
| JS/TS | 90% | **Full** |
| Java | 80% | **Full** |
| Kotlin | 0% | **Full** (shares Java) |
| Go | 0% | **Full** |
| C | 30% | **Full** |
| C++ | 0% | **Full** (shares C) |
| Ruby | 10% | **Full** |
| Swift | 0% | **Full** |
| C# | 0% | **Full** |

**Total: 27 tasks, ~5400 lines across 7 plans.**

**11 languages at full support** — enum, runner, instrumentor, sandbox, index, detectors, call graph.
