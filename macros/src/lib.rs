extern crate proc_macro;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, parse_macro_input};

#[proc_macro_derive(Encode)]
pub fn encode_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident; // The name of the struct, e.g., `Cursor`

    let encode_impl =
        match input.data {
            Data::Struct(data) => {
                let fields = match data.fields {
                    Fields::Named(fields) => fields.named,
                    Fields::Unnamed(_) => {
                        return syn::Error::new_spanned(
                            name,
                            "Tuple structs are not yet supported for Encode",
                        )
                        .to_compile_error()
                        .into();
                    }
                    Fields::Unit => {
                        return syn::Error::new_spanned(
                            name,
                            "Unit structs are not yet supported for Encode",
                        )
                        .to_compile_error()
                        .into();
                    }
                };

                let encoding = fields.iter().map(|f| {
                    let field_name = &f.ident;
                    quote! {
                        encoder = encoder.append(&self.#field_name);
                    }
                });

                quote! {
                    impl crate::storage::encdec::Encode for #name {
                        fn encode(&self) -> Vec<u8> {
                            let mut encoder = crate::storage::encdec::EncodeBuilder::new();

                            #(#encoding)*

                            encoder.build()
                        }
                    }
                }
            }
            Data::Enum(data_enum) => {
                let variant_encodings = data_enum.variants.iter().enumerate().map(
                |(index, variant)| {
                    let variant_name = &variant.ident;
                    let variant_index = index as u8; // Encode as first byte

                    match &variant.fields {
                        Fields::Unit => {
                            // Unit variant, like `Bitcoin`.
                            quote! {
                                Self::#variant_name => vec![#variant_index]
                            }
                        }
                        Fields::Unnamed(fields) => {
                            // Tuple variant, like `Rune((u64, u32))`.
                            let field_names: Vec<_> = (0..fields.unnamed.len())
                                .map(|i| format_ident!("field{}", i))
                                .collect();

                            quote! {
                                Self::#variant_name((#(#field_names),*)) => {
                                    vec![vec![#variant_index], #(#field_names.encode()),*].concat()
                                }
                            }
                        }
                        Fields::Named(fields) => {
                            // Struct-like variant, like `SomeVariant { a: A, b: B }`.
                            let field_names: Vec<_> = fields.named.iter()
                                .map(|f| f.ident.as_ref().unwrap())
                                .collect();

                            quote! {
                                Self::#variant_name { #(#field_names),* } => {
                                    vec![vec![#variant_index], #(#field_names.encode()),*].concat()
                                }
                            }
                        }
                    }
                });

                quote! {
                    impl crate::storage::encdec::Encode for #name {
                        fn encode(&self) -> Vec<u8> {
                            match self {
                                #(#variant_encodings),*
                            }
                        }
                    }
                }
            }
            _ => {
                return syn::Error::new_spanned(name, "Encode only supports structs and enums")
                    .to_compile_error()
                    .into();
            }
        };

    encode_impl.into()
}

#[proc_macro_derive(Decode)]
pub fn decode_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident; // The name of the struct, e.g., `Cursor`

    let decode_impl = match input.data {
        Data::Struct(data) => {
            let fields = match data.fields {
                Fields::Named(fields) => fields.named,
                Fields::Unnamed(_) => {
                    return syn::Error::new_spanned(
                        &name,
                        "Tuple structs are not yet supported for Decode",
                    )
                    .to_compile_error()
                    .into();
                }
                Fields::Unit => {
                    return syn::Error::new_spanned(
                        &name,
                        "Unit structs are not yet supported for Decode",
                    )
                    .to_compile_error()
                    .into();
                }
            };

            // Collect field names for struct reconstruction
            let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();
            let field_decodes = fields.iter().map(|f| {
                let field_name = &f.ident;
                let field_ty = &f.ty;

                quote! {
                    let (#field_name, rest) = <#field_ty as crate::storage::encdec::Decode>::decode(bytes)?;
                    bytes = rest;  // Update the slice to the remaining bytes after decoding
                }
            });

            quote! {
                impl crate::storage::encdec::Decode for #name {
                    fn decode(bytes: &[u8]) -> crate::DecodingResult<Self> {
                        let mut bytes = bytes;  // Mutable reference to slice

                        #(#field_decodes)*
                        Ok((Self {
                            #(#field_names: #field_names),*
                        }, bytes))
                    }
                }
            }
        }
        Data::Enum(data_enum) => {
            // Match the first byte and decode the corresponding variant.
            let variant_decodings =
                data_enum
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| {
                        let variant_name = &variant.ident;
                        let variant_index = index as u8; // Variant index, which was encoded as the first byte

                        // Use `quote!` to generate the literal for the variant index
                        let variant_index_literal = quote! { #variant_index };

                        match &variant.fields {
                            Fields::Unit => {
                                // Unit variant, like `Bitcoin`, no fields to decode
                                quote! {
                                    #variant_index_literal => {
                                        Ok((Self::#variant_name, bytes))
                                    }
                                }
                            }
                            Fields::Unnamed(fields) => {
                                // Tuple variant, like `Rune((u64, u32))`
                                let field_names: Vec<_> = (0..fields.unnamed.len())
                                    .map(|i| format_ident!("field{}", i))
                                    .collect();

                                // Dealing with the fields' decoding
                                let field_decodes = fields.unnamed.iter().enumerate().map(|(i, _)| {
                                    let field_ty = &fields.unnamed[i].ty;
                                    let field_name = &field_names[i];
                                    quote! {
                                        let (#field_name, bytes) = <#field_ty as crate::storage::encdec::Decode>::decode(bytes)?;
                                    }
                                });

                                quote! {
                                    #variant_index_literal => {
                                        #(#field_decodes)*
                                        Ok((Self::#variant_name(#(#field_names),*), bytes))
                                    }
                                }
                            }
                            Fields::Named(fields) => {
                                // Struct-like variant, like `SomeVariant { a: A, b: B }`
                                let field_names: Vec<_> = fields
                                    .named
                                    .iter()
                                    .map(|f| f.ident.as_ref().unwrap())
                                    .collect();

                                let field_decodes = fields.named.iter().map(|f| {
                                    let field_ty = &f.ty;
                                    let field_name = &f.ident;
                                    quote! {
                                        let (#field_name, bytes) = <#field_ty as crate::storage::encdec::Decode>::decode(bytes)?;
                                    }
                                });

                                quote! {
                                    #variant_index_literal => {
                                        #(#field_decodes)*
                                        Ok((Self::#variant_name { #(#field_names),* }, bytes))
                                    }
                                }
                            }
                        }
                    });

            // Wrap the implementation into the final output for the `Decode` trait
            quote! {
                impl crate::storage::encdec::Decode for #name {
                    fn decode(bytes: &[u8]) -> crate::DecodingResult<Self> {
                        if bytes.is_empty() {
                            return Err(crate::DecodingError::MalformedInput("enum insufficient bytes".to_string(), bytes.to_vec()))
                        }

                        let kind = bytes[0];
                        let bytes = &bytes[1..];
                        match kind {
                            // For each variant, match the index byte and decode accordingly
                            #(#variant_decodings)*
                            _ => Err(crate::DecodingError::InvalidEnumKind(bytes.to_vec())),
                        }
                    }
                }
            }
        }
        _ => {
            return syn::Error::new_spanned(name, "Decode only supports structs and enums")
                .to_compile_error()
                .into();
        }
    };

    decode_impl.into()
}
