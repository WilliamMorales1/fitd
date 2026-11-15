/**
 * Tree-sitter grammar for fitd diagram sources. Mirrors parse.rs and the macro lines of macros.rs: one
 * statement per line, blocks in `{ }`, attributes in `[ ]`, `//` comments; `#` right after `=` starts a color.
 */
const KEYWORDS = ['title', 'root', 'layout', 'page', 'font', 'theme', 'style', 'node', 'key', 'group',
  'include', 'require', 'prefer', 'with', 'for', 'if', 'else', 'def'];

module.exports = grammar({
  name: 'fitd',

  extras: $ => [/[ \t\r]/, $.comment],

  word: $ => $.name,

  // A lone id may be a node or a qualified name; the token after it tells.
  conflicts: $ => [[$.node_statement, $.qualified_name]],

  rules: {
    source_file: $ => seq(repeat($._line), optional($._statement)),
    _line: $ => seq(optional($._statement), $._newline),
    // A block may end with a statement on the `}` line, as in `a { b }`.
    block: $ => seq('{', repeat($._line), optional($._statement), '}'),

    _newline: _ => '\n',

    _statement: $ => choice(
      $.title_statement,
      $.root_statement,
      $.layout_statement,
      $.page_statement,
      $.font_statement,
      $.theme_statement,
      $.style_statement,
      $.key_statement,
      $.node_statement,
      $.name_statement,
      $.with_statement,
      $.body,
      $.figure,
      $.edge_statement,
      $.group_statement,
      $.constraint_statement,
      $.include_statement,
      $.for_statement,
      $.if_statement,
      $.def_statement,
      $.row_statement,
    ),

    title_statement: $ => seq('title', $.string),
    root_statement: $ => seq('root', field('node', $.name)),
    layout_statement: $ => seq('layout', field('kind', $.name), optional($.attributes)),
    page_statement: $ => seq('page', field('size', $._value), repeat($.flag), optional($.attributes)),
    font_statement: $ => seq('font', field('size', $._value), optional(seq('..', field('max', $._value))), optional($.attributes)),
    theme_statement: $ => seq('theme', optional(field('preset', $.name)), optional($.attributes)),
    key_statement: $ => seq('key', field('label', $.string), optional($.attributes)),
    style_statement: $ => seq('style', field('name', $.qualified_name), optional($.attributes)),
    // A node line may drop `node` when a title or attributes follow the id.
    node_statement: $ => seq(
      choice(
        seq('node', field('id', $.name), optional(field('title', $.string)), optional($.attributes)),
        seq(field('id', $.name), choice(seq(field('title', $.string), optional($.attributes)), $.attributes)),
      ),
      optional($.block),
    ),
    // A lone name is a bare node, a reopened one, or a template called without arguments; only sema and macros.rs know which.
    name_statement: $ => seq(field('name', $.qualified_name), optional($.block)),
    with_statement: $ => seq('with', field('style', $.qualified_name), optional($.block)),
    body: _ => token(seq('|', /[^\n]*/)),
    figure: _ => token(seq('~', /[^\n]*/)),
    edge_statement: $ => seq(
      // A node may be named like a keyword; the edge operator after it tells. No source: from the node above.
      optional(seq($._source, repeat(seq(',', $._source)))),
      repeat1($._hop),
      optional($.attributes),
    ),
    // A hop's targets feed the next hop: `a -> b, c "x" -- d`.
    _source: $ => field('from', choice($.name, alias(choice(...KEYWORDS), $.name))),
    _hop: $ => seq(field('operator', $.edge_operator), $._targets, optional(field('label', $.string))),
    group_statement: $ => seq('group', optional(seq(optional(field('id', $.name)), field('title', $.string))), $._targets, optional($.attributes)),
    constraint_statement: $ => seq(choice('require', 'prefer'), $._relation, optional($.attributes)),
    include_statement: $ => seq('include', field('path', $.string)),

    for_statement: $ => seq('for', field('var', $.name), optional(seq(',', field('var', $.name))), 'in', field('items', $.expression), $.block),
    if_statement: $ => seq(
      'if', field('condition', $.expression), $.block,
      repeat(seq('else', 'if', field('condition', $.expression), $.block)),
      optional(seq('else', $.block)),
    ),
    def_statement: $ => seq('def', field('name', $.name), '(', optional(seq(field('parameter', $.name), repeat(seq(',', field('parameter', $.name))))), ')', $.block),
    // A template call, or a row of a template block; only macros.rs knows which names are templates.
    row_statement: $ => prec.dynamic(-1, seq(field('name', choice($.qualified_name, $.string, $.number, $.word)), repeat1(field('argument', $._row_word)), optional($.block))),
    _row_word: $ => choice($.qualified_name, $.string, $.number, $.word),
    word: _ => token(prec(-1, /[^\s"#=.{}\[\]]+(\.[^\s"#=.{}\[\]]+)*/)),
    expression: _ => token(prec(-1, /([^\n\/{]|\/[^\n\/{])+/)),

    edge_operator: _ => choice('->', '--', '..>', '..'),
    _targets: $ => seq(field('to', $.name), repeat(seq(optional(','), field('to', $.name)))),

    _relation: $ => choice($.align, $.placement, $.chain),
    align: $ => seq('align', field('side', $.name), '(', $.name, repeat(seq(',', $.name)), ')'),
    placement: $ => seq($.name, 'at', field('place', $.name)),
    chain: $ => seq($.name, repeat1(seq(field('relation', $.relation), $.name))),
    relation: $ => $.name,

    flag: $ => $.name,
    qualified_name: $ => seq($.name, optional(seq('.', $.name))),
    // Newlines inside brackets do not end the statement.
    attributes: $ => seq('[', repeat($._newline), optional(seq($.attribute, repeat(seq(',', repeat($._newline), $.attribute)), repeat($._newline))), ']'),
    attribute: $ => seq(field('key', $.name), '=', field('value', choice($._value, $.color))),
    _value: $ => choice($.name, $.string, $.number),

    // `${expr}` may stand anywhere in a name, as macros write names like `s${i}`.
    name: _ => /([\p{L}_]|\$\{[^}\n]*\})([\p{L}\p{N}_]|\$\{[^}\n]*\})*(-([\p{L}\p{N}_]|\$\{[^}\n]*\})+)*/,
    // Quotes inside `${}` belong to the expression, as in `"${part(xs, ";", i)}"`.
    string: _ => /"([^"\\\n$]|\$\{[^}\n]*\}|\$|\\[^\n])*"/,
    number: _ => /[0-9]([\p{L}\p{N}]|[.:][0-9])*/,
    color: _ => token.immediate(prec(1, /#[0-9a-fA-F]*/)),
    comment: _ => token(seq('//', /[^\n]*/)),
  },
});
