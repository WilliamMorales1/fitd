; A later pattern overrides an earlier one, so the catch-all comes first.
(name) @variable

["title" "root" "layout" "page" "font" "theme" "style" "node" "key" "group" "require" "prefer" "include" "align" "at"] @keyword
["for" "in" "if" "else" "end" "def"] @keyword.control

(edge_operator) @operator
["=" ".." "." "," "(" ")"] @punctuation.delimiter

(comment) @comment
(string) @string
(body) @string.documentation
(figure) @string.special
(number) @number
(color) @constant

(attribute key: (name) @property)
(block_statement style: (qualified_name (name) @type))
(style_statement name: (name) @type)
(flag (name) @constant.builtin)
(relation (name) @keyword.operator)
(align side: (name) @keyword.operator)
(placement place: (name) @constant.builtin)
(layout_statement kind: (name) @constant.builtin)
(theme_statement preset: (name) @constant.builtin)

(node_statement id: (name) @label)
(def_statement name: (name) @function)
(row_statement name: (qualified_name (name) @function.call))
(def_statement parameter: (name) @variable.parameter)
(expression) @embedded
