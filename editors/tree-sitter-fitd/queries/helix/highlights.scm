; A later pattern overrides an earlier one, so the catch-all comes first.
(name) @variable

["title" "root" "layout" "page" "font" "theme" "style" "with" "key" "group" "require" "prefer" "include" "align" "at"] @keyword
["for" "in" "if" "else" "def"] @keyword.control

(edge_operator) @operator
["=" ".." "." "," "(" ")" "[" "]" "{" "}"] @punctuation.delimiter

(comment) @comment
(string) @string
(body) @string.special
(number) @constant.numeric
(color) @constant

(attribute key: (name) @variable.other.member)
(with_statement style: (qualified_name (name) @type))
(name_statement name: (qualified_name (name) @label))
(style_statement name: (qualified_name (name) @type))
(flag (name) @constant.builtin)
(relation (name) @keyword.operator)
(align side: (name) @keyword.operator)
(placement place: (name) @constant.builtin)
(layout_statement kind: (name) @constant.builtin)
(theme_statement preset: (name) @constant.builtin)

(node_statement id: (name) @label)
(def_statement name: (name) @function)
(call_statement name: (qualified_name (name) @function.call))
(argument key: (name) @variable.parameter)
(def_statement parameter: (name) @variable.parameter)
(expression) @embedded
