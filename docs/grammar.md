# Axiom grammar

The authoritative grammar lives in the Rust parser under
`stage1/crates/axiomc/src/syntax.rs`. This document is a compact guide to the
currently supported source shape.

```ebnf
program        := item* EOF ;

item           := macro_item
               | import_item
               | const_item
               | type_item
               | struct_item
               | enum_item
               | trait_item
               | impl_item
               | fn_item ;

macro_item     := ("macro" | "macro_rules!") IDENT "{" macro_arm (";" macro_arm)* "}" ;
macro_arm      := "(" macro_param_list? ")" "=>" macro_template ;
macro_template := "{" source_text "}" | "(" source_text ")" ;
macro_param_list := macro_param ("," macro_param)* ;
macro_param    := "$" IDENT (":" IDENT)?
               | "$(" "$" IDENT (":" IDENT)? ")" separator? "*" ;
separator      := "," ;
import_item    := "import" STRING ;
const_item     := visibility? "const" IDENT ":" type "=" expr ;
generic_params := "<" type_param ("," type_param)* ">" ;
type_param     := IDENT (":" IDENT ("+" IDENT)*)? ;
type_item      := visibility? "type" IDENT generic_params? "=" type ;
struct_item    := visibility? "struct" IDENT generic_params? "{" fields? "}" ;
enum_item      := visibility? "enum" IDENT generic_params? "{" variants? "}" ;
trait_item     := visibility? "trait" IDENT "{" trait_method* "}" ;
trait_method   := "fn" IDENT "(" params? ")" ":" type ";"? ;
impl_item      := "impl" IDENT "for" IDENT "{" fn_item* "}"
               | "impl" IDENT "{" fn_item* "}" ;
fn_item        := visibility? "fn" IDENT generic_params? "(" params? ")" ":" type block ;
visibility     := "pub" | "pub(pkg)" ;
lifetime       := "'" IDENT ;

stmt           := let_stmt
               | print_stmt
               | return_stmt
               | if_stmt
               | while_stmt
               | match_stmt
               | expr ;

let_stmt       := "let" IDENT (":" type)? "=" expr ;
print_stmt     := "print" expr ;
return_stmt    := "return" expr ;
if_stmt        := "if" expr block ("else" block)? ;
while_stmt     := "while" expr block ;
match_stmt     := "match" expr "{" match_arm+ "}" ;
match_arm      := IDENT match_payload? block ;
match_payload  := "(" IDENT ("," IDENT)* ")"
               | "{" IDENT ("," IDENT)* "}" ;
block          := "{" stmt* "}" ;

type           := IDENT type_args?
               | "[" type "]"
               | "&" lifetime? "[" type "]"
               | "&" lifetime? "mut" "[" type "]"
               | "map" "[" type "," type "]"
               | "(" type ("," type)+ ")" ;

expr           := literal
               | IDENT
               | call
               | struct_literal
               | enum_literal
               | array_literal
               | map_literal
               | tuple_literal
               | match_expr
               | expr binary_op expr
               | expr "?"
               | expr "." IDENT
               | expr "." INT
               | expr "[" expr "]"
               | "&" expr "[" range? "]" ;
```

Comments start with `#` and run to end-of-line. See
[stage1.md](stage1.md) for the current implementation scope and known gaps.
Pattern guards and nested destructuring patterns are not supported in the
current stage1 parser.

Declarative `macro` and compatibility `macro_rules!` support is intentionally
small in stage1: top-level definitions, explicit `$name` captures, one repeated
`$($name:fragment),*` capture per arm, textual expansion before type-check, and
a bounded recursive expansion depth. Macro output may invoke other macros and
the expander repeats until no invocations remain or the active recursion limit
is exceeded. `axiomc check --macro-recursion-limit <n>` adjusts the default
limit of 64. Multi-line expansions must be invoked as a whole statement;
single-line expansions can appear inside expressions. `axiomc check --json`
includes `macro_expansions` metadata when a checked package expands macros.

Trait declarations support required method signatures, explicit generic bounds
such as `<T: Eq + Tagged>`, `impl Trait for Type` blocks, and static method
dispatch from bounded generic functions. Trait default bodies, supertraits,
generic impl headers, blanket impls, trait type positions, and `dyn Trait`
dynamic dispatch are rejected in stage1.
