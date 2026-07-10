; Top-level functions, methods, and type declarations (struct/interface/alias). Each
; definition node carries a KIND capture; the identifier carries @name.

(source_file
  (function_declaration name: (identifier) @name) @function)

(source_file
  (method_declaration name: (field_identifier) @name) @method)

(source_file
  (type_declaration
    (type_spec name: (type_identifier) @name)) @type)
