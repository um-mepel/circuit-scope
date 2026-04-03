// Intentionally malformed header to exercise diagnostics and recovery.
module bad_header(
    input a
    output b // missing comma and closing parenthesis / semicolon

// body and endmodule still present so parser should recover and find the module
  assign b = a;
endmodule

