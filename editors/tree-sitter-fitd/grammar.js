/**
 * Tree-sitter grammar for fitd diagram sources. Mirrors the EBNF in docs/LANGUAGE.md: one
 * statement per line, `#` comments except right after `=`, where it starts a color.
 */
module.exports = grammar({
  name: 'fitd',

  extras: $ => [/[ \t\r]/, $.comment],

  word: $ => $.name,

  // A name after a target or chain link may be the next one or an attribute key; only the
  // `=` after it tells, one token too late for LR(1), so both are explored.
  conflicts: $ => [[$._targets], [$.chain]],

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
      $.node_statement,
      $.body,
      $.edge_statement,
      $.group_statement,
      $.constraint_statement,
      $.include_statement,
    ),

    title_statement: $ => seq('title', $.string),
    root_statement: $ => seq('root', field('node', $.name)),
    layout_statement: $ => seq('layout', field('kind', $.name), repeat($.attribute)),
    page_statement: $ => seq('page', field('size', $._value), repeat(choice($.flag, $.attribute))),
    font_statement: $ => seq('font', field('size', $._value), optional(seq('..', field('max', $._value))), repeat($.attribute)),
    theme_statement: $ => seq('theme', optional(field('preset', $.name)), repeat($.attribute)),
    style_statement: $ => seq('style', field('name', $.name), repeat($.attribute)),
    node_statement: $ => seq('node', field('id', $.name), optional(field('title', $.string)), repeat(choice($.class, $.attribute))),
    body: _ => token(seq('|', /[^\n]*/)),
    edge_statement: $ => seq(
      field('from', $.name),
      field('operator', $.edge_operator),
      $._targets,
      optional(field('label', $.string)),
      repeat(choice($.class, $.attribute)),
    ),
    group_statement: $ => seq('group', optional(field('title', $.string)), $._targets, repeat($.attribute)),
    constraint_statement: $ => seq(choice('require', 'prefer'), $._relation, repeat($.attribute)),
    include_statement: $ => seq('include', field('path', $.string)),

    edge_operator: _ => choice('->', '--', '..>', '..'),
    _targets: $ => seq(field('to', $.name), repeat(seq(optional(','), field('to', $.name)))),

    _relation: $ => choice($.align, $.placement, $.chain),
    align: $ => seq('align', field('side', $.name), '(', $.name, repeat(seq(',', $.name)), ')'),
    placement: $ => seq($.name, 'at', field('place', $.name)),
    chain: $ => seq($.name, repeat1(seq(field('relation', $.relation), $.name))),
    relation: $ => $.name,

    flag: $ => $.name,
    class: $ => seq('.', $.name),
    attribute: $ => seq(field('key', $.name), '=', field('value', choice($._value, $.color))),
    _value: $ => choice($.name, $.string, $.number),

    name: _ => /[\p{L}_][\p{L}\p{N}_]*(-[\p{L}\p{N}_]+)*/,
    string: _ => /"([^"\\\n]|\\[^\n])*"/,
    number: _ => /[0-9]([\p{L}\p{N}]|\.[0-9])*/,
    color: _ => token.immediate(prec(1, /#[0-9a-fA-F]*/)),
    comment: _ => token(seq('#', /[^\n]*/)),
  },
});
