; A later pattern overrides an earlier one, so the catch-all comes first.
(name) @variable

["title" "root" "layout" "page" "font" "theme" "style" "node" "group" "require" "prefer" "include" "align" "at"] @keyword

(edge_operator) @operator
["=" ".." "." "," "(" ")"] @punctuation.delimiter

(comment) @comment
(string) @string
(body) @string.documentation
(number) @number
(color) @constant

(attribute key: (name) @property)
(class (name) @type)
(style_statement name: (name) @type)
(flag (name) @constant.builtin)
(relation (name) @keyword.operator)
(align side: (name) @keyword.operator)
(placement place: (name) @constant.builtin)
(layout_statement kind: (name) @constant.builtin)
(theme_statement preset: (name) @constant.builtin)

(node_statement id: (name) @label)
