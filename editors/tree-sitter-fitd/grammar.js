/**
 * Tree-sitter grammar for fitd diagram sources. Mirrors parse.rs and the macro lines of macros.rs: one
 * statement per line, `#` comments except right after `=`, where it starts a color.
 */
const KEYWORDS = ['title', 'root', 'layout', 'page', 'font', 'theme', 'style', 'node', 'key', 'group',
  'include', 'require', 'prefer', 'for', 'if', 'else', 'end', 'def'];

module.exports = grammar({
  name: 'fitd',

  extras: $ => [/[ \t\r]/, $.comment],

  word: $ => $.name,

  // A name after a target or chain link may be the next one or an attribute key; only the
  // `=` after it tells, one token too late for LR(1), so both are explored.
  conflicts: $ => [[$._targets], [$.chain], [$.node_statement, $.qualified_name]],

  rules: {
    source_file: $ => seq(repeat(seq(optional($._statement), $._newline)), optional($._statement)),

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
      $.block_statement,
      $.body,
      $.figure,
      $.edge_statement,
      $.group_statement,
      $.constraint_statement,
      $.include_statement,
      $.for_statement,
      $.if_statement,
      $.else_statement,
      $.end_statement,
      $.def_statement,
      $.row_statement,
    ),

    title_statement: $ => seq('title', $.string),
    root_statement: $ => seq('root', field('node', $.name)),
    layout_statement: $ => seq('layout', field('kind', $.name), repeat($.attribute)),
    page_statement: $ => seq('page', field('size', $._value), repeat(choice($.flag, $.attribute))),
    font_statement: $ => seq('font', field('size', $._value), optional(seq('..', field('max', $._value))), repeat($.attribute)),
    theme_statement: $ => seq('theme', optional(field('preset', $.name)), repeat($.attribute)),
    key_statement: $ => seq('key', field('label', $.string), repeat($.attribute)),
    style_statement: $ => seq('style', field('name', $.name), repeat($.attribute)),
    // A node line may drop `node` when a title or attribute follows the id; indentation nests it, which only parse.rs tracks.
    node_statement: $ => choice(
      seq('node', field('id', $.name), optional(field('title', $.string)), repeat($.attribute)),
      seq(field('id', $.name), choice(seq(field('title', $.string), repeat($.attribute)), repeat1($.attribute))),
    ),
    // A lone name opens a style block over the lines indented under it; inside a block an undeclared one is a node, which only sema knows.
    block_statement: $ => field('style', $.qualified_name),
    body: _ => token(seq('|', /[^\n]*/)),
    figure: _ => token(seq('~', /[^\n]*/)),
    edge_statement: $ => seq(
      // A node may be named like a keyword; the edge operator after it tells. No source: from the node above.
      optional(seq($._source, repeat(seq(',', $._source)))),
      repeat1($._hop),
      repeat($.attribute),
    ),
    // A hop's targets feed the next hop: `a -> b, c "x" -- d`.
    _source: $ => field('from', choice($.name, alias(choice(...KEYWORDS), $.name))),
    _hop: $ => seq(field('operator', $.edge_operator), $._targets, optional(field('label', $.string))),
    group_statement: $ => seq('group', optional(field('title', $.string)), $._targets, repeat($.attribute)),
    constraint_statement: $ => seq(choice('require', 'prefer'), $._relation, repeat($.attribute)),
    include_statement: $ => seq('include', field('path', $.string)),

    // Statements stay flat, one per line; nesting is left to `end` for macros and indentation for style blocks.
    for_statement: $ => seq('for', field('var', $.name), optional(seq(',', field('var', $.name))), 'in', field('items', $.expression)),
    if_statement: $ => seq('if', field('condition', $.expression)),
    else_statement: $ => seq('else', optional(seq('if', field('condition', $.expression)))),
    end_statement: _ => 'end',
    def_statement: $ => seq('def', field('name', $.name), '(', optional(seq(field('parameter', $.name), repeat(seq(',', field('parameter', $.name))))), ')'),
    // A template call, or a row of a template block; only macros.rs knows which names are templates.
    row_statement: $ => prec.dynamic(-1, seq(field('name', choice($.qualified_name, $.string, $.number, $.word)), repeat1(field('argument', $._row_word)))),
    _row_word: $ => choice($.name, $.string, $.number, $.word),
    word: _ => token(prec(-1, /[^\s"#=.]+(\.[^\s"#=.]+)*/)),
    expression: _ => token(prec(-1, /[^\n#]+/)),

    edge_operator: _ => choice('->', '--', '..>', '..'),
    _targets: $ => seq(field('to', $.name), repeat(seq(optional(','), field('to', $.name)))),

    _relation: $ => choice($.align, $.placement, $.chain),
    align: $ => seq('align', field('side', $.name), '(', $.name, repeat(seq(',', $.name)), ')'),
    placement: $ => seq($.name, 'at', field('place', $.name)),
    chain: $ => seq($.name, repeat1(seq(field('relation', $.relation), $.name))),
    relation: $ => $.name,

    flag: $ => $.name,
    qualified_name: $ => seq($.name, optional(seq('.', $.name))),
    attribute: $ => seq(field('key', $.name), '=', field('value', choice($._value, $.color))),
    _value: $ => choice($.name, $.string, $.number),

    // `${expr}` may stand anywhere in a name, as macros write names like `s${i}`.
    name: _ => /([\p{L}_]|\$\{[^}\n]*\})([\p{L}\p{N}_]|\$\{[^}\n]*\})*(-([\p{L}\p{N}_]|\$\{[^}\n]*\})+)*/,
    // Quotes inside `${}` belong to the expression, as in `"${part(xs, ";", i)}"`.
    string: _ => /"([^"\\\n$]|\$\{[^}\n]*\}|\$|\\[^\n])*"/,
    number: _ => /[0-9]([\p{L}\p{N}]|[.:][0-9])*/,
    color: _ => token.immediate(prec(1, /#[0-9a-fA-F]*/)),
    comment: _ => token(seq('#', /[^\n]*/)),
  },
});
