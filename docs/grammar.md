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
               | fn_item ;

macro_item     := "macro_rules!" IDENT "{" macro_arm "}" ;
macro_arm      := "(" macro_param_list? ")" "=>" "{" source_text "}" ;
macro_param_list := macro_param ("," macro_param)* ;
macro_param    := "$" IDENT (":" IDENT)? ;
import_item    := "import" STRING ;
const_item     := visibility? "const" IDENT ":" type "=" expr ;
type_item      := visibility? "type" IDENT generic_params? "=" type ;
struct_item    := visibility? "struct" IDENT generic_params? "{" fields? "}" ;
enum_item      := visibility? "enum" IDENT generic_params? "{" variants? "}" ;
fn_item        := visibility? "fn" IDENT generic_params? "(" params? ")" ":" type block ;
visibility     := "pub" | "pub(pkg)" ;

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
               | "&" "[" type "]"
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

Declarative `macro_rules!` support is intentionally small in stage1: one arm per
macro, explicit `$name` captures, textual expansion before type-check, and a
bounded recursive expansion depth. Multi-line expansions must be invoked as a
whole statement; single-line expansions can appear inside expressions.
