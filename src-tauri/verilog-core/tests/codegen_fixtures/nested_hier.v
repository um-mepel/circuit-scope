module inverter(
    input a,
    output y
);
    assign y = ~a;
endmodule

module buffer(
    input a,
    output y
);
    wire inv_out;
    inverter u_inv(.a(a), .y(inv_out));
    inverter u_inv2(.a(inv_out), .y(y));
endmodule

module top_nested(
    input in1,
    output out1
);
    buffer u_buf(.a(in1), .y(out1));
endmodule
