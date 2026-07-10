; Top-level functions and classes, plus methods declared directly in a class body.
; Each definition node carries a KIND capture; the identifier carries @name.

(module
  (function_definition name: (identifier) @name) @function)

(module
  (class_definition name: (identifier) @name) @class)

(module
  (decorated_definition
    definition: (function_definition name: (identifier) @name) @function))

(module
  (decorated_definition
    definition: (class_definition name: (identifier) @name) @class))

(class_definition
  body: (block
    (function_definition name: (identifier) @name) @method))

(class_definition
  body: (block
    (decorated_definition
      definition: (function_definition name: (identifier) @name) @method)))
