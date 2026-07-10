; Top-level items and methods declared inside an `impl` block. Each definition node
; carries a KIND capture; the identifier carries @name.

(source_file
  (function_item name: (identifier) @name) @function)

(source_file
  (struct_item name: (type_identifier) @name) @struct)

(source_file
  (enum_item name: (type_identifier) @name) @enum)

(source_file
  (trait_item name: (type_identifier) @name) @trait)

(impl_item
  body: (declaration_list
    (function_item name: (identifier) @name) @method))
