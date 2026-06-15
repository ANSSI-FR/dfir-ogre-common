use crate::{Field, FieldName, Record, Value, errors::Error};

pub(crate) trait FieldTraversalHandler {
    fn visit_flat_field(
        &mut self,
        input_data: &mut Record,
        field_name: &FieldName,
        ignore: bool,
        root: bool,
    );

    fn visit_object_field(
        &mut self,
        input_data: &mut Record,
        field_name: &FieldName,
        fields: &[Field],
        ignore: bool,
        force_snake_case: bool,
    ) -> Result<(), Error>;

    fn visit_array_field(
        &mut self,
        input_data: &mut Record,
        inner_field: &Field,
        force_snake_case: bool,
    ) -> Result<(), Error>;

    fn visit_unmapped_field(
        &mut self,
        key: String,
        value: Value,
        force_snake_case: bool,
    ) -> Result<(), Error>;
}

pub(crate) fn traverse_fields<H>(
    input_data: &mut Record,
    fields: &[Field],
    handler: &mut H,
    force_snake_case: bool,
    root: bool,
) -> Result<(), Error>
where
    H: FieldTraversalHandler,
{
    for field in fields {
        match field {
            Field::Single {
                name,
                parser: _,
                default_value: _,
            } => {
                handler.visit_flat_field(input_data, name, field.ignore(), root);
            }
            Field::Multi(multi_input_field) => {
                handler.visit_flat_field(
                    input_data,
                    &multi_input_field.output_field,
                    field.ignore(),
                    root,
                );
            }
            Field::Array(array_field) => {
                handler.visit_array_field(input_data, array_field.0.as_ref(), force_snake_case)?;
            }
            Field::Object {
                name,
                fields,
                ignore,
            } => {
                handler.visit_object_field(input_data, name, fields, *ignore, force_snake_case)?;
            }
        }
    }

    let unmapped: Vec<(String, Value)> = input_data.drain().collect();
    for (key, value) in unmapped {
        handler.visit_unmapped_field(key, value, force_snake_case)?;
    }

    Ok(())
}
