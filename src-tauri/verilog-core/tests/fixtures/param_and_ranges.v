// Parameterized module with typed and ranged ports.
module param_and_ranges #(
    parameter WIDTH = 8
) (
    input  [WIDTH-1:0] data_in,
    output [WIDTH-1:0] data_out,
    inout  [3:0]       ctrl
);
endmodule

