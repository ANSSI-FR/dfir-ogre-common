from datetime import datetime
from typing import Any, Dict, List, Optional

COMPRESSION_LEVEL: str

class ArrayField:
    """A wrapper for a field that represents an array in the data model."""

    def __init__(self, field: Field) -> None:
        """Initialize the ArrayField with a given Field.

        Args:
            field: The Field instance to wrap.
        """
        ...

class BatchEntry:
    """a batch entry used by the OgreBatchPlugin"""
    file: str
    run_config: RunConfiguration
    metadata: Metadata

    def __init__(self, file: str, run_config: RunConfiguration, metadata: Metadata) -> None:
         ...

class DateInputCodec:
    """Defines supported input formats for parsing date strings"""

    @classmethod
    def Iso(cls):
        """A codec that parses date strings in ISO 8601 format (e.g., '2023-12-31T23:59:59')."""
        ...

    @classmethod
    def FileTime(cls):
        """A codec that parses date strings as Windows File Time in seconds (e.g., 133210364868558102)."""
        ...

    @classmethod
    def Timestamp(cls):
        """A codec that parses date strings as Unix timestamps in seconds (e.g., 1703654400)."""
        ...

    @classmethod
    def TimestampMs(cls):
        """A codec that parses date strings as Unix timestamps in milliseconds (e.g., 1704067199000)."""
        ...

    @classmethod
    def TimestampNs(cls):
        """A codec that parses date strings as Unix timestamps in nanosecond (e.g., 1703664000123456789)."""
        ...

    @classmethod
    def Pattern(cls, value: str):
        """A codec that parses date strings using the given format pattern (e.g., '%Y-%m-%d %H:%M:%S')."""
        ...

class DateOutputCodec:
    """Defines supported output formats for serializing date objects"""

    @classmethod
    def Iso(cls):
        """A codec that serializes date objects into ISO 8601 format (e.g., '2023-12-31T23:59:59')."""
        ...

    @classmethod
    def IsoUtc(cls):
        """A codec that serializes date objects into ISO 8601 format with UTC timezone (e.g., '2023-12-31T23:59:59Z')."""
        ...

    @classmethod
    def UtcNaive(cls):
        """A codec that serializes date objects into ISO 8601 format without timezone, assuming UTC (e.g., '2023-12-31T23:59:59')."""
        ...

    @classmethod
    def Pattern(cls, value: str):
        """A codec that serializes date objects using the given format pattern (e.g., '%Y-%m-%d %H:%M:%S')."""
        ...

class Field:
    """Represents different types of fields in the data model.
    Contains three variants: simple fields, multi-fields, and object fields.
    Each variant defines how data should be parsed and transformed."""

    @classmethod
    def Single(cls, name: FieldName, parser: Parser, default_value: Optional[str]):
        """Create a field with a single input field with a given name and parser.

        Args:
            name: The name of the field.
            parser: A parser object responsible for transforming input data into the desired field value.
            default_value: a value that will be used when the input is empty

        """
        ...

    @classmethod
    def Multi(cls, value: MultiInputField):
        """Create a field that requires multiple input
        Args:
            value: A MultiInputField object that defines the structure and parsing behavior for the multi-field.

        """
        ...

    @classmethod
    def Array(cls, value: ArrayField):
        """Represents an array field in the data model.
        This variant is used to handle collections of values, where each element
        is processed using the `ArrayField` configuration.
        """
        ...

    @classmethod
    def Object(cls, name: FieldName, fields: list[Field], ignore: bool = False):
        """Create an object field with a given name and a list of nested fields.

        This variant allows for hierarchical data structures by grouping multiple fields under a single name.

        Args:
            name: The name of the object field.
            fields: A list of Field instances that define the nested structure within the object.

        """
        ...

class FieldName:
    """Stores field metadata including names, qualifiers, and descriptions.
    This is used to map input fields to output names with optional
    qualifiers and documentation."""

    def __init__(
        self,
        input_name: str,
        output_name: Optional[str] = None,
        qualifier: Optional[str] = None,
        display_name: Optional[str] = None,
        description: Optional[str] = None,
    ) -> None:
        """Initialize a FieldName instance.

        Args:
            input_name: The name of the field in the input data.
            output_name: The name of the field in the output data. If not provided, defaults to input_name.
            qualifier: An optional qualifier
            description: An optional human-readable description of the field.
        """
        ...

    def input_name(self) -> str:
        """Return the name of the input field"""
        ...

    def output_name(self) -> str:
        """The output field name as a string. If no output name was specified, returns the input name."""
        ...

    def name(self, with_qualifier: bool) -> str:
        """Return the field name, optionally including the qualifier.

        Args:
            with_qualifier: If True, include the qualifier in the returned name. If False, return only the base name.
        """
        ...

    def display(self) -> str:
        """Return a human-readable short description of the field."""
        ...

    def describe(self) -> str:
        """Return a human-readable description of the field."""
        ...

class FieldMapping:
    """Manages field mappings for data parsing, organizing fields into a structured hierarchy.
    This struct facilitates the conversion of input data into a Record by maintaining
    a mapping of field names to their parsing configurations.

    Attributes:
        mapping: A hierarchical index map representing the structure of fields and their parsers.
        field_parsers: A collection of field parsers organized for efficient lookup and processing.
    """

    def __init__(
        self, mapping: List[Field], default_parser: Optional[Parser] = None
    ) -> None:
        """Creates a new FieldMapping instance from a list of Field definitions.

        Args:
            mapping: A vector of Field definitions that describe the structure of the data.
            default_parser: An optional parser to use as a fallback for fields without explicit mappings.
        """
        ...

    def get_field_parser_tree(self) -> FieldParserTree:
        """Returns a clone of the internal FieldParserTree instance.

        This method provides access to the parser hierarchy built from the field mappings,
        allowing for programmatic lookup of parsers by field name or path.
        """
        ...

    def get_parser(self, input_name: str) -> Optional[FieldParser]:
        """Retrieves a parser for a specific field name.

        This method first checks for an exact match of the input name. If not found,
        it uses the default parser if available. It does not support nested object paths.

        Args:
            input_name: The name of the field to retrieve a parser for

        Returns:
            A FieldParser instance if found, otherwise None
        """
        ...

    def get_parser_by_path(self, path: List[str]) -> Optional[FieldParser]:
        """Retrieves a parser for a nested field path.

        This method supports accessing nested fields through a vector of names.
        For example, `["System", "TimeCreated", "SystemTime"]` would navigate
        through nested objects to find a parser.

        Args:
            path: A list of field names representing the nested path

        """
        ...

    def get_parser_subtree(self, path: List[str]) -> Optional[FieldParserTree]:
        """Retrieves a nested `FieldParserTree` for a specific field name, if it exists.

        This method allows access to the parser hierarchy for nested object fields. This is useful for working with hierarchical data structures where
        fields contain sub-fields that need to be parsed recursively.

        Args:
            input_name: The name of the field to look up in the parser registry.
        """
        ...

class FieldParserTree:
    """Organizes field parsers from the `FieldMapping` into a hierarchical structure, supporting both direct field lookups and nested path-based access.

    It also handles fallback parsing using a default parser when specific field mappings are missing.

    Attributes:
        parsers: A map from field names to their corresponding parser types (either direct parsers or nested objects)
        default_parser: An optional parser used as a fallback for fields without explicit mappings
    """

    def get_output_name(self) -> str:
        """Retrieve the output name for this 'FieldParserTree'"""
        ...

    def get_parser(self, input_name: str) -> Optional[FieldParser]:
        """Retrieves a parser for a specific field name.

        This method first checks for an exact match of the input name. If not found,
        it uses the default parser if available. It does not support nested object paths.

        Args:
            input_name: The name of the field to retrieve a parser for

        Returns:
            A FieldParser instance if found, otherwise None
        """
        ...

    def get_parser_by_path(self, path: List[str]) -> Optional[FieldParser]:
        """Retrieves a parser for a nested field path.

        This method supports accessing nested fields through a vector of names.
        For example, `["System", "TimeCreated", "SystemTime"]` would navigate
        through nested objects to find a parser.

        Args:
            path: A list of field names representing the nested path

        Returns:
            A FieldParser instance if found, otherwise None
        """
        ...

    def get_parser_subtree(self, input_name: str) -> Optional[FieldParserTree]:
        """Retrieves a nested `FieldParserTree` for a specific field name, if it exists.

        This method allows access to the parser hierarchy for nested object fields. This is useful for working with hierarchical data structures where
        fields contain sub-fields that need to be parsed recursively.

        Args:
            input_name: The name of the field to look up in the parser registry.
        """
        ...

class FieldParser:
    """FieldParser is a struct that manages parsing of input fields into a Record.
    It holds the input name and a list of Field parsers."""

    def parse(self, input: Optional[str], output: Record) -> None:
        """Parses the input string using the registered fields and populates the output Record.

        This method applies each registered field parser to the input string, extracting and
        transforming the corresponding data, then writes the results into the provided output Record
        at the appropriate indices.

        Args:
            input: The input string containing the data to be parsed.
            output: A Record that will be populated with the parsed values in the order defined by the field parsers.
        """
        ...

    def parse_into_value(self, input: Optional[str]) -> Optional[Value]:
        """Parses the input string and returns the first non-None value from the registered fields.

        Args:
            input: The input string to be parsed.

        Returns:
            Optional[Value]: The first non-None value from the parsed fields, or None if no value was found.
        """

    def set_value(self, value: Value, output: Record) -> None:
        """Sets the provided value into the output with the correct name.

        Args:
            value: The Value to be set in the output record.
            output: A mutable reference to the Record where values will be stored.
        """

    def input_name(self) -> str:
        """Return the name of the input field that this parser is associated with."""
        ...

class FilesToExtract:
    """
    Holds a mapping from an archive entry path to the desired output filename.
    """

    def __init__(self) -> None: ...
    """Create a new empty ``FilesToExtract`` instance."""

    def add(self, input_path: str, output_path: str) -> None: ...
    """
    Register a file to be extracted.

    Parameters
    ----------
    input_path: str Path of the entry inside the archive.
    output_path: str Filename (relative to the output folder) where the entry should be written.
    """

    def len(self) -> int:...
    """
    Returns the number of files to be extracted
    """


class FileReport:
    """Result structure containing file information"""

    output_type: str
    format: str
    date_format: str
    with_timeline: bool
    with_qualifiers: bool
    include_empty: bool
    file_name: str
    num_lines: int

class Metadata:
    """ metadata associated with a file or artifact.
     Fields are optional except for `computer`, which identifies the host system."""
    computer: str
    data_type: str
    id: Optional[str]
    orc_id: Optional[str]
    folder: Optional[str]
    archive: Optional[str]
    subarchive: Optional[str]
    archive_filename: Optional[str]
    original_filename:Optional[str]
    vss: Optional[str]
    orc_start_date: Optional[datetime]
    creation_date: Optional[datetime]
    modif_date: Optional[datetime]

    def __init__(self,computer:str ) -> None: ...


class MultiInputField:
    """Support cases where a single output value depends on the combined
    processing of multiple input fields, such as concatenating strings, computing derived values,
    or applying conditional logic across several inputs.
    """

    def __init__(
        self,
        input_fields: List[Field],
        output_field: FieldName,
        parser: MultiParser,
    ) -> None:
        """
        Args:
            input_fields: A list of Field instances that define the input fields to be processed.
            output_field: A FieldName object specifying the output name, optional qualifier, and description.
            parser: A MultiParser instance that defines how the input fields are combined into a single output.
        """
        ...

class MultiParser:
    """Defines multi-input field parsing strategies.
    Currently implements a join parser that combines multiple inputs
    into a single string value."""

    @classmethod
    def Join(cls, separator: str, trim: bool = False):
        """This parser combines the values from multiple input fields into one string,
        using the specified separator between values. Optionally trims whitespace
        from each input before joining.
        Args:
            separator: The string to use as a delimiter between input values.
            trim: If True, removes leading and trailing whitespace from each input value before joining.

        """
        ...

class OgrePlugin:
    """A minimal base class for plugins in the dfir-ogre-common project,
    allowing Python users to subclass and implement custom behavior (e.g., parse() logic).

    Subclasses **must** implement the `parse` and `description` methods.
    """
    def description(self) -> PluginDescription:
        """Return a description of the plugin's purpose and functionality.

        Returns:
            A PluginDescription object containing human-readable details about the plugin.
        """
        ...

    def parse(
        self,
        input_file: str,
        plugin_file: str,
        run_config: RunConfiguration,
        metadata: Metadata,
    ) -> RunReport:
        """Execute the parsing logic for the given input file using the provided configuration and metadata.

        This method is the core entry point for plugin-based data processing. It processes
        the input file according to the configuration, applies any required transformations,
        and generate output files.

        Args:
            input_file: The path to the input file to be parsed.
            plugin_file: the path of the plugin xml configuration,
            configuration: A RunConfiguration object containing settings and parameters for parsing.
            metadata: A dictionary of metadata associated with the input file (e.g., source, timestamp, origin).
        """
        ...

class OgreBatchedPlugin:
    """A base class for plugins that process multiple input files in a single batch.

    This class extends the plugin pattern to handle batched input, where each
    file carries its own configuration and metadata via :class:`BatchEntry`
    objects. Unlike :class:`OgrePlugin`, which processes a single file per call,
    this class receives a list of batch entries and is responsible for
    orchestrating their processing as a group.

    Subclasses **must** implement the :meth:`parse` and :meth:`description`
    methods.
    """
    def description(self) -> PluginDescription:
      """Return a description of the plugin's purpose and functionality.

      Returns:
          A PluginDescription object containing human-readable details about the plugin.
      """
      ...

    def parse(
        self,
        input_files: List[BatchEntry],
        plugin_file: str,
    ) -> RunReport:
      """Execute the parsing logic for the given batch of input files.

      This method is the core entry point for batched plugin-based data
      processing. It iterates over the provided :class:`BatchEntry` list,
      each of which contains a file path, its :class:`RunConfiguration`,
      and associated :class:`Metadata`, and processes them according to
      the plugin's logic.

      Args:
          input_files: A list of :class:`BatchEntry` objects, each specifying
              the path to an input file along with its run configuration and
              metadata (e.g., source, timestamp, origin).
          plugin_file: The path to the plugin XML configuration file.

      Returns:
          A :class:`RunReport` containing the results of the batch processing.
      """
      ...



class Output:
    """
    Main output handler that manages multiple output types
    """

    def __init__(
        self,
        run_config: RunConfiguration,
        plugin_config: PluginConfiguration,
        metadata: Metadata,
        data_type: Optional[str] = None,
    ):
        """
        Creates a new Output instance with the given configuration

        Args:
            configuration: The run configuration containing output settings.
            metadata: Optional dictionary of metadata associated with the output (e.g., source, timestamp).
            field_mapping: Optional field mapping configuration that defines how input fields are transformed.
            timeline_builder: Optional timeline builder.
        """
        ...

    def get_report(self) -> OutputReport:
        """
        Retrieves information about the files that have been written

        Returns:
            `OuputResult` - Contains the file name and the number of lines written
        """
        ...

    def write(self, data: Record):
        """Writes the provided data to all configured output destinations.

        This method processes the Record of parsed values and writes them to the configured outputs
        based on the field mapping and configuration.
        Args:
            data: A Record containing parsed values corresponding to the expected output fields.
        """
        ...
    def __enter__(self) -> Output: ...
    def __exit__(
        self,
        exc_type: Any,
        exc_value: Any,
        exc_tb: Any,
    ) -> None: ...

class OutputConfiguration:
    """This structure defines how data should be formatted.
    It includes settings for serialization format, date formatting,
    and various flags for including additional data."""

    output_type: str
    format: str
    date_format: str
    with_timeline: bool
    with_qualifiers: bool
    include_empty: bool
    output_folder: str
    base_file_name: str
    params: Dict[str, str]

    def __init__(
        self,
        base_file_name: str,
        output_folder: str,
        output_type: str = "file",
        format: str = "jsonl",
        date_format: str = "iso_utc",
        with_timeline: bool = False,
        with_qualifiers: bool = False,
        include_empty: bool = True,
        params: Dict[str, str] = {},
    ) -> None:
        """Create a new ``OutputConfiguration``.
        Args:
            base_file_name: Base name for created output files (extension is appended automatically).
            output_folder: Directory where output files will be written.
            output_type: Identifier for the output kind. Defaults to ``"file"``.
            format: Primary serialization format, e.g. ``"json"`` or ``"csv"``. Defaults to ``"jsonl"``.
            date_format: Pattern used for serialising timestamps, e.g. ``"iso_utc"``. Defaults to ``"iso_utc"``.
            with_timeline: Include timeline information when ``True``. Defaults to ``False``.
            with_qualifiers: Attach qualifier data to field names when ``True``. Defaults to ``False``.
            include_empty: Emit empty fields when ``True``. Defaults to ``True``.
            params: Free-form key/value pairs for extra options. Defaults to ``{}``.
        """
        ...

    def __deepcopy__(self, _memo: Any):
        """Return a deep copy of this OutputConfiguration instance.

        Args:
            _memo: A dictionary used by the deepcopy module to track already copied objects.

        Returns:
            A new OutputConfiguration instance with identical settings.
        """
        ...

class OutputReport:
    """Result structure containing output information"""

    last_error: Optional[str]
    num_errors: int
    file_reports: List[FileReport]

class ParserExtension:
    """Wrappping class for rust parser extension"""

    ...

def parse_csv(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: int,
) -> RunReport:
    """
    Parse a CSV file using the specified configuration and mapping.

    Args:
        input_file: Path to the input CSV file
        run_config: Configuration for the parsing run
        metadata: Metadata associated with the parsing operation
        csv_config_file: Path to the CSV configuration file
        python_mapping: defines custom Python Parser for input column names
        rust_mapping_extension: define custom mapping for rust parser extensions,
        log_before_fail: Number of errors to log before failing

    Returns:
        RunReport containing the parsing results or error information

    """
    ...

def parse_evtx(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport:
    """Parses an EVTX file

    Args:
        input_file: Path to the EVTX file to be parsed.
        configuration: The `RunConfiguration` that defines how the output should be structured.
        metadata: Metadata associated with the parsing process.

    Returns:
        RunReport: The result of the parsing operation
    """
    ...

def parse_hive_keys(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    root_name: Optional[str] = None,
    regexp: Optional[str] = None,
) -> RunReport:
    """Extract Keys from a Windows Hive file

    Parameters
    ----------
    input_file: Path to the Hive file to be parsed.
    configuration: RunConfiguration specifying parsing settings.
    metadata: Metadata associated with the parsing task.
    root_name: Optional string to prepend to paths in the output.
    regexp: Optional regexp to filter path.

    """
    ...

def parse_json(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport:
    """Parses an JSON file

    Args:
        input_file: Path to the SRUM database file.
        configuration: Configuration for the parsing run.
        metadata: Metadata associated with the parsing operation.

    """
    ...

def parse_jsonl(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport:
    """Parses an JSONL file (each line is a valid json object)

    Args:
        input_file: Path to the SRUM database file.
        configuration: Configuration for the parsing run.
        metadata: Metadata associated with the parsing operation.

    """
    ...

def parse_regexp(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: int,
) -> RunReport:
    """
    Parsing files using regexp.

    Processes each line of the file, extracting fields using a regex patterns and constructing data records based on a parsing schema.
    Lines that does not match the regexp can be ignored, cause an error, or be agregated with the last field of the previous line

    Args:
        input_file: Path to the log file to be parsed
        run_config: Configuration for the parsing run
        metadata: Metadata associated with the input file
        regexp_config_file: Path to the configuration file defining the parsing schema
        log_before_fail: Number of errors to log before failing
    """
    ...

def parse_sqlite(
    input_file: str,
    run_donfig: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: int,
) -> RunReport:
    """
    Parses an SQLite database file using the query provide in the sqlite configuration file.

    Args:
        input_file: Path to the SQLite database file
        configuration: Run configuration parameters
        metadata: Metadata associated with the run
        sqlite_config_file: Path to the configuration file defining the sql query and data mapping
        log_before_fail: Number of errors to log before failing

    """
    ...

def parse_srum(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport:
    """Parses a SRUM (System Resource Usage Monitor) database file.

    Args:
        input_file: Path to the SRUM database file.
        configuration: Configuration for the parsing run.
        metadata: Metadata associated with the parsing operation.

    """
    ...

def parse_xml(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport:
    """Parses an XML file

    Args:
        input_file: Path to the SRUM database file.
        configuration: Configuration for the parsing run.
        metadata: Metadata associated with the parsing operation.

    """
    ...

class Parser:
    """Defines parsing strategies for different data types.
    This enum contains various parsing methods including basic types,
    date parsing, string splitting, and Python-based parsing.
    """
    @classmethod
    def Ignore(cls):
        """A parser that ignores the input field entirely, resulting in no output."""
        ...

    @classmethod
    def Dynamic(cls, parsers: List[Parser]):
        """try each parsers in the list in order and returns the first successfull parse"""
        ...

    @classmethod
    def Int(cls):
        """A parser that converts the input field to an integer value."""
        ...

    @classmethod
    def IntRadix(cls, radix: int):
        """A parser that converts the input field to an integer using the specified radix (base).

        Args:
            radix: The base (radix) for the integer conversion (e.g., 2 for binary, 16 for hexadecimal).
        """
        ...

    @classmethod
    def IntToHex(cls, width: int):
        """parse the input as a signed 64-bit integer and convert it to hexadecimal string with configurable width for zero-padding.
        - width = 0: no padding, width > 0: pad with leading zeros
        """
        ...

    @classmethod
    def Float(cls):
        """A parser that converts the input field to a floating-point number."""
        ...

    @classmethod
    def Bool(cls):
        """A parser that converts the input field to a boolean value.
        Treats 'false', '0', 'no', 'n', '','off' as False, everything else is considered to be True"""
        ...

    @classmethod
    def String(cls):
        """A parser that preserves the input field as a string without modification."""
        ...

    @classmethod
    def StringToLower(cls):
        """A parser that convert the input string to lowercase"""
        ...

    @classmethod
    def StringToUpper(cls):
        """A parser that convert the input string to uppercase"""
        ...

    @classmethod
    def DateTime(cls, codec: DateInputCodec):
        """A parser that converts the input field into a datetime object using the specified date codec.

        Args:
            codec: A DateInputCodec instance that defines how to interpret the input date string.
        """
        ...

    @classmethod
    def Split(cls, separator: str):
        """A parser that splits the input field into a list of strings using the given separator.

        Args:
            separator: The string used to split the input field.
        """
        ...

    @classmethod
    def Python(cls, parser: PyParser):
        """A parser that executes a custom Python function to transform the input.

        Args:
            parser: A callable (function or callable object) that takes a string input and returns the parsed value.
        """
        ...

class PluginConfiguration:
    """This every mapping available for a specific Plugin"""

    plugin: str
    data_type_configs: List[DataTypeMapping]

    @classmethod
    def load(
        cls,
        input_file: str,
        python: Optional[Dict[str, Any]] = None,
        extension: Optional[Dict[str, ParserExtension]] = None,
    ) -> "PluginConfiguration": ...

    def get_data_type_mapping(self, data_type: Optional[str] = None) -> DataTypeMapping:
      """Returns the DataTypeMapping for the specified data type.

      If `data_type` is `None`, first data type is returned.

      :param data_type: Optional string specifying the data type to look up.
      :raises Error: If the data type is not found.
      """
      ...

    def get_parsers(
        self,
        data_type: Optional[str] = None,
    ) -> Optional[FieldParserTree]:
        """
        Returns the field parser tree for the specified data type.

        If `data_type` is `None`, the parser tree for the first configured data type is returned.

        * `data_type` – Optional string specifying the data type to look up.
        """
        ...

class DataTypeMapping:
    """This class defines the mapping data for a specific data type"""

    data_type: str
    description: Optional[str]
    file_encoding: Optional[str]
    default_date_pattern: DateInputCodec
    params: Dict[str, str]
    timeline: Optional[TimeLineBuilder]
    field_mapping: Optional[FieldMapping]

class PluginDescription:
    """
    This class is used to provide metadata about a plugin's purpose and usage, particularly for CLI tools.
    """
    def __init__(self, command: str, description: str):
        """Initialize a PluginDescription instance.

        Args:
            command: The command name used to invoke the plugin (e.g., 'parse-json').
            description: A human-readable description of the plugin's functionality.
        """
        ...

    def get_command(self) -> str:
        """Return the command name associated with this plugin."""
        ...

    def get_description(self) -> str:
        """Return the human-readable description of the plugin."""
        ...

class PyParser:
    """Wraps Python objects for integration with Rust code via pyo3.
    This is used to delegate parsing operations to Python implementations."""

    def __init__(self, parser: AbstractParser): ...

class AbstractParser:
    """define function required to implement a custom python parser"""

    def parse(self, input: str, ouput_name: str) -> Optional[Record]: ...
    def output_fields_names(self) -> List[FieldName]: ...

class RegValue:
    """Represents a registry value and its data"""

    def name(self) -> str:
        """Returns the name of the registry value"""
        ...

    def type(self) -> str:
        """Returns the data type of the registry value as a string"""
        ...

    def data(self) -> Any:
        """Gets the data of the registry value as a Python object"""
        ...

    def to_record(self) -> Record:
        """Converts the registry value to a Record representation"""
        ...

class RegKey:
    """Represents a registry key and its values"""

    mtime: datetime
    security_descriptor: (
        SecurityDescriptor  # Assuming SecurityDescriptor is represented as Any
    )
    path: str
    name: str

    def sub_keys(self) -> List["RegKey"]:
        """Gets every sub keys of this registry key

        Returns:
            A list of RegKey instances
        """
        ...

    def sub_key(self, path: str) -> Optional["RegKey"]:
        """Gets a specific subkey by name

        Args:
            path: The name of the subkey to find

        Returns:
            A RegKey instance if found, None otherwise
        """
        ...

    def sub_path(self, path: str) -> Optional["RegKey"]:
        """Gets a specific subkey by path

        Args:
            path: The path of the subkey to find

        Returns:
            A RegKey instance if found, None otherwise
        """
        ...

    def sub_glob(self, path: str) -> List["RegKey"]:
        """Gets subkeys that match a glob pattern

        Args:
            path: The glob pattern to match

        Returns:
            A list of RegKey instances that match the pattern
        """
        ...

    def value(self, name: str) -> Optional["RegValue"]:
        """Gets a specific value by name

        Args:
            name: The name of the value to retrieve

        Returns:
            The RegValue instance if found, None otherwise
        """
        ...

    def values(self) -> List["RegValue"]:
        """Gets every registry values"""
        ...

    def value_data(self, name: str, default: Optional[Any] = None) -> Optional[Any]:
        """Gets the data of a specific value

        Args:
            name: The name of the value to retrieve data for

        Returns:
            The data of the value if found, None otherwise
        """
        ...

    def to_record(self) -> Record:
        """Converts the registry key to a record representation

        Returns:
            A record containing the registry key's properties
        """
        ...

class Registry:
    """A class representing a registry hive."""

    @classmethod
    def load(cls, input_file: str, root_name: str) -> "Registry":
        """Loads a registry from a file.

        Args:
            input_file: Path to the registry hive file.
            root_name: The root name of the keys.
        """
        ...

    def glob_keys(self, path: str) -> List["RegKey"]:
        """Searches for registry keys matching a pattern.

        Args:
            path: The path pattern to search for.

        Returns:
            A list of RegKey instances matching the pattern.
        """
        ...

class RunConfiguration:
    """This structure holds all the parameters needed to execute the parsing operations.
    It includes the output name, configured output settings, and additional parameters."""

    output: List[OutputConfiguration]
    force_snake_case: bool
    params: Dict[str, Optional[str]]
    def __init__(
        self,
        output: List[OutputConfiguration],
        force_snake_case: bool = False,
        params: Optional[Dict[str, Optional[str]]] = None,
    ):
        """
        Args:
            output: A list of OutputConfiguration objects that define the output formats and settings.
            force_snake_case: whether to rename unmapped field in snake case
            params: A dictionary of additional configuration parameters, where keys are parameter names and values are optional strings.
        """
        ...

class TimeLineType:
    """Enumeration of supported timeline types for timestamp interpretation .

    - MacbMacb: A timeline format that includes height timestamps that provides two M-A-C-B, one for the standard file timestamps and one for the pe timestamps
    - Macb: A timeline format that includes the four timestamps (M, A, C, B) in the order M-A-C-B, commonly used in file system forensics.
    - Standard: A standard timeline format that accumulates field description.
    """

    MacbMacb: Any
    Macb: Any
    Standard: Any

class TimelineDisplayOptions:
    include_field_name: bool
    field_separator: str
    """
    Display Options for the description and additional description fields.
    """

class TimeLineBuilder:
    """A builder for constructing timeline data structures used in forensic and event analysis.

    This class manages the configuration and field definitions required to generate timeline entries,
    including related users, descriptions, and additional metadata. It supports nested field paths
    and integrates with timeline types such as MacbMacb, Macb, and Standard to provide structured
    timestamp interpretations.

    The builder allows defining field paths (e.g., 'data.user') and associating them with specific
    field types like related user or description, which are later used to populate timeline output.
    """
    def __init__(
        self,
        timeline_type: TimeLineType,
        time_zone: str,
        source_type: str,
        data_type: str,
        max_date_meaning: int = 0,
        description_format: Optional[TimelineDisplayOptions] = None,
        additional_description_format: Optional[TimelineDisplayOptions] = None,
    ):
        """Initialize a new TimeLineBuilder instance.

        Args:
            timeline_type: The type of timeline to use (e.g., MacbMacb, Macb, Standard).
            time_zone: The time zone string (e.g., "UTC", "America/New_York").
            source_type: The type of source that generated the data (e.g., "file", "evtx").
            data_type: The type of data being processed (e.g., "ntfs", "registry").
            max_date_meaning: The maximum number of field descriptions to include in the meaning of a timestamp.
        """
        ...
    def add_related_user_ouput_name(self, value: str) -> None:
        """Add a field name to be used for related user metadata in the timeline."""
        ...

    def add_related_user_ouput_path(self, path: List[str]) -> None:
        """Add a path (e.g., ['data', 'user']) to be used for related user metadata in the timeline.

        Args:
            path: A list of strings representing the path to the related user field.
        """
        ...

    def add_description_ouput_name(self, value: str) -> None:
        """Add a field name to be used for the primary description in the timeline."""
        ...

    def add_description_ouput_path(self, path: List[str]) -> None:
        """Add a path (e.g., ['data', 'desc']) to be used for the primary description in the timeline.

        Args:
            path: A list of strings representing the path to the description field.
        """
        ...

    def add_additional_description_ouput_name(self, value: str) -> None:
        """Add a field name to be used for additional description metadata in the timeline."""
        ...

    def add_additional_description_ouput_path(self, path: List[str]) -> None:
        """Add a path (e.g., ['data', 'extra']) to be used for additional description metadata in the timeline.

        Args:
            path: A list of strings representing the path to the additional description field.
        """
        ...

class Record:
    """A wrapper for key-value pairs used in data serialization and parsing.
    This struct is primarily used to represent objects in the data model."""

    def __init__(self):
        """Initialize an empty Record instance."""
        ...

    def add(self, name: str, value: Value):
        """Add a key-value pair to the record.

        Args:
            name: The name of the field.
            value: The value to associate with the field.
        """
        ...

    def clear(self):
        """Removes all entries from the internal map, leaving the `Record` empty, while conserving the map capacity."""
        ...

    def len(self) -> int:
        """returns the size of this record"""
        ...

    def to_string(self) -> str: ...

class Value:
    """Represents various data types that can be serialized.

    This enum supports a wide range of value types used in structured data processing, including
    null values, strings, arrays, integers, floats, booleans, dates, and nested objects.

    Examples:
        - `Value::String("hello")` represents a string value.
        - `Value::Int(42)` represents an integer.
        - `Value::Date(datetime)` represents a timestamp in UTC.
        - `Value::Object(Record)` represents a nested object with key-value pairs.
    """
    @classmethod
    def Null(cls): ...
    @classmethod
    def String(cls, v: str): ...
    @classmethod
    def Array(cls, v: List[Value]): ...
    @classmethod
    def Int(cls, v: int): ...
    @classmethod
    def Float(cls, v: float): ...
    @classmethod
    def Bool(cls, v: bool): ...
    @classmethod
    def Date(cls, v: datetime): ...
    @classmethod
    def Object(cls, v: Record): ...

class RunReport:
    """Report for the parser execution results.

    Attributes:
        error: Optional error message from the parser execution.
        output_result: List of output results from the parser execution.
    """

    last_error: Optional[str]
    num_errors: int
    output_reports: List[OutputReport]

    def __init__(self) -> None: ...
    def add_error(self, error: str): ...
    def add_output_report(self, output_report: OutputReport) -> None:
        """Add output results to the report.

        Args:
            output_result: List of output results to append.
        """
        ...

class SecurityDescriptor:
    """Represents a Windows Security Descriptor with access control information."""

    def __init__(self) -> None:
        self.control_flags: List[str] = []
        self.owner_sid: str = ""
        self.group_sid: str = ""
        self.sacl_ace: Optional["SecurityDescriptorAce"] = None
        self.dacl_ace: Optional["SecurityDescriptorAce"] = None

    def to_record(self) -> Record:
        """Converts the SecurityDescriptor to a Record containing its attributes."""
        ...

class SecurityDescriptorAce:
    """Represents an Access Control Entry (ACE) within a Security Descriptor."""

    def __init__(self) -> None:
        self.ace_type: Optional[str] = None
        self.ace_flags: List[str] = []
        self.rights: Optional[List[str]] = None
        self.account_sid: str = ""

    def to_record(self) -> Record:
        """Converts the SecurityDescriptorAce to a Record containing its attributes."""
        ...

def extract_7z_files(
    archive_path: str,
    files: FilesToExtract,
    output_folder: str,
    password: Optional[str] = None,
) -> None: ...
"""
Extract selected files from a 7‑zip archive.

Parameters
----------
archive_path: str - Path to the ``*.7z`` file.
files: FilesToExtract - Mapping of entries to extract.
output_folder: str - Directory where extracted files are written.
password: Optional[str] - Optional password for encrypted archives.

Raises
------
OSError
    If the archive cannot be opened or read.
RuntimeError
    If extraction fails for any other reason (e.g., bad password).
"""

def extract_7z_file(
    archive_path: str,
    file: str,
    output_folder: str,
    password: Optional[str] = None,
) -> None: ...
"""
Extract selected file from a 7‑zip archive.

Parameters
----------
archive_path: str - Path to the ``*.7z`` file.
file: the file to extract
output_folder: str - Directory where extracted files are written.
password: Optional[str] - Optional password for encrypted archives.

Raises
------
OSError
    If the archive cannot be opened or read.
RuntimeError
    If extraction fails for any other reason (e.g., bad password).
"""

def security_descriptor_from_bytes(b: bytes) -> SecurityDescriptor:
    """Constructs a SecurityDescriptor from binary data.

    Args:
        b: Byte string containing the serialized security descriptor.

    Returns:
        A SecurityDescriptor instance populated with data from the byte string.
    """
    ...

def win_frn_hex_parser(prefix: str) -> ParserExtension:
    """Dispatch the content of the frn field on the 'sequence' and 'record' fields."""
    ...

def win_frn_int_parser(prefix: str) -> ParserExtension:
    """Dispatch the content of the frn field on the 'sequence' and 'record' fields."""
    ...

def win_ntfs_flag_parser() -> ParserExtension:
    """Transform the NTFSInfo Attributes into a list of boolean fields"""
    ...

def win_signed_hash_parser() -> ParserExtension:
    """Put SignedHash value into the right hash field"""
    ...

class Qualifiers:
    """
    Qualifiers are optional labels or tags that can be appended to field names to provide additional context

    Example:
        A field named 'timestamp' with qualifier 'DATE_CREATION' would be rendered as 'timestamp:creation_date'
        in the output when qualifiers are included.
    """

    # Timestamp
    DATE_CREATION: str
    DATE_MODIFICATION: str
    DATE_CHANGE: str
    DATE_ACCESS: str
    DATE_COMPILATION: str
    DATE_INSTALLATION: str
    DATE_UNINSTALL: str
    DATE_LAST_RUN: str
    TIMEZONE: str

    # Computer
    COMPUTER_NAME: str

    # OS
    OS_VERSION: str
    OS_ARCH: str

    # User
    USER_NAME: str
    USER_SID: str
    USER_ID: str
    LOGON_ID: str

    # Group
    GROUP_ID: str
    GROUP_NAME: str

    # Filesystem
    FS_INODE: str
    FS_USN: str
    VOLUME_GUID: str
    MFT_SEQUENCE: str

    # Disk
    DISK_SIZE: str

    # File
    FILE_NAME: str
    FILE_SIZE: str
    FILE_PATH: str
    FILE_PATH_SHA1: str
    FILE_MD5: str
    FILE_SHA1: str
    FILE_SHA256: str
    FILE_SHA384: str
    FILE_SHA512: str
    FILE_TIGER: str
    FILE_WHIRLPOOL: str
    FILE_SSDEEP: str
    FILE_TLSH: str
    FILE_ATTRS: str

    # PE
    PE_MD5: str
    PE_SHA1: str
    PE_SHA256: str
    PE_ARCH: str
    PE_SUBSYSTEM: str
    PE_VERSION: str
    EXIT_CODE: str

    # File execution
    COMMAND_LINE: str

    # Application
    APP_ID: str
    APP_NAME: str
    APP_CLSID: str
    MSI_PRODUCT: str
    MSI_PACKAGE: str

    # Publisher
    COMPANY: str
    PUBLISHER: str
    PRODUCT: str

    # Certificate
    CERT_SHA1: str

    # Registry
    HIVE_MOUNT: str
    KEY_NAME: str
    KEY_PATH: str
    VALUE_NAME: str
    VALUE_DATA: str

    # Service
    SERVICE_NAME: str
    SERVICE_TYPE: str
    SERVICE_DISPLAY_NAME: str
    SERVICE_START_TYPE: str

    # Process
    PROCESS_ID: str

    # ScheduledTask
    SCHTASK_GUID: str
    SCHTASK_URI: str

    # Event
    EVT_PROVIDER: str
    EVT_ID: str
    EVT_CHANNEL: str
    EVT_RECORD_ID: str

    # State
    IN_USE: str
    REUSE_COUNT: str

    # DNS
    DOMAIN_NAME: str

    # Windows
    WINDOWS_PRIVILEGES: str
    SECURITY_DESCRIPTOR: str
    WINDOWS_OBJECT: str

    # Network
    IP_ADDRESS: str
    IP_PORT: str
    MAC_ADDRESS: str

    def __init__(self): ...
