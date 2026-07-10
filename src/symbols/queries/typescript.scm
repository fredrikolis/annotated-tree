; Top-level (and `export`-ed) functions, classes, interfaces, and type aliases, plus
; class methods. Each definition node carries a KIND capture; the identifier @name.

(program
  (function_declaration name: (identifier) @name) @function)

(program
  (class_declaration name: (type_identifier) @name) @class)

(program
  (interface_declaration name: (type_identifier) @name) @interface)

(program
  (type_alias_declaration name: (type_identifier) @name) @type)

(program
  (export_statement
    (function_declaration name: (identifier) @name) @function))

(program
  (export_statement
    (class_declaration name: (type_identifier) @name) @class))

(program
  (export_statement
    (interface_declaration name: (type_identifier) @name) @interface))

(program
  (export_statement
    (type_alias_declaration name: (type_identifier) @name) @type))

(class_declaration
  body: (class_body
    (method_definition name: (property_identifier) @name) @method))
