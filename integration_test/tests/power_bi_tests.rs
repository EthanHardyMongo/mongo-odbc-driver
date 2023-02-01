mod common;

mod integration {
    use crate::common::{
        generate_default_connection_str, get_sql_diagnostics, sql_return_to_string,
    };
    use odbc::ffi::SQL_NTS;
    use odbc_sys::{
        AttrConnectionPooling, AttrOdbcVersion, ConnectionAttribute, DriverConnectOption,
        EnvironmentAttribute, HDbc, HEnv, Handle, HandleType, InfoType, Pointer, SQLAllocHandle,
        SQLDriverConnectW, SQLFreeHandle, SQLGetInfoW, SQLSetConnectAttrW, SQLSetEnvAttr, SmallInt,
        SqlReturn,
    };
    use std::ptr::null_mut;
    use std::slice;
    use widechar::WideChar;

    const BUFFER_LENGTH: SmallInt = 300;

    macro_rules! test_get_info {
        ($conn_handle:expr, $info_type: expr, $info_value_buffer_length: expr, $info_value_type: expr) => {{
            let conn_handle = $conn_handle;
            let info_type = $info_type;
            let info_value_buffer_length = $info_value_buffer_length;
            let info_value_type = $info_value_type;

            let output_buffer = &mut [0u16; (BUFFER_LENGTH as usize - 1)] as *mut _;
            let mut buffer = OutputBuffer {
                output_buffer: output_buffer as Pointer,
                data_length: *&mut 0,
            };

            let outcome = SQLGetInfoW(
                conn_handle as HDbc,
                info_type,
                buffer.output_buffer,
                info_value_buffer_length,
                &mut buffer.data_length as &mut _,
            );
            assert_eq!(
                SqlReturn::SUCCESS,
                outcome,
                "Expected {}, got {}. Diagnostic message is: {}",
                sql_return_to_string(SqlReturn::SUCCESS),
                sql_return_to_string(outcome),
                get_sql_diagnostics(HandleType::Dbc, conn_handle as Handle)
            );

            let length = buffer.data_length.clone();
            println!(
                "{info_type:?} = {}\nLength is {length}",
                match info_value_type {
                    DataType::WChar => Into::<String>::into(buffer),
                    DataType::USmallInt => Into::<u16>::into(buffer).to_string(),
                }
            );
        }};
    }

    pub enum DataType {
        USmallInt,
        WChar,
    }

    pub struct OutputBuffer {
        pub output_buffer: Pointer,
        pub data_length: i16,
    }

    impl From<OutputBuffer> for String {
        fn from(val: OutputBuffer) -> Self {
            unsafe {
                String::from_utf16_lossy(slice::from_raw_parts(
                    val.output_buffer as *const _,
                    val.data_length as usize / std::mem::size_of::<WideChar>(),
                ))
            }
        }
    }

    impl From<OutputBuffer> for u16 {
        fn from(val: OutputBuffer) -> Self {
            unsafe { *(val.output_buffer as *mut u16) }
        }
    }

    /// Setup flow.
    /// This will allocate a new environment handle and set ODBC_VERSION and CONNECTION_POOLING environment attributes.
    /// Setup flow is:
    ///     - SQLAllocHandle(SQL_HANDLE_ENV)
    ///     - SQLSetEnvAttr(SQL_ATTR_ODBC_VERSION, SQL_OV_ODBC3)
    ///     - SQLSetEnvAttr(SQL_ATTR_CONNECTION_POOLING, SQL_CP_ONE_PER_HENV)
    fn setup() -> odbc_sys::HEnv {
        let mut env: Handle = null_mut();

        unsafe {
            assert_eq!(
                SqlReturn::SUCCESS,
                SQLAllocHandle(HandleType::Env, null_mut(), &mut env as *mut Handle)
            );

            assert_eq!(
                SqlReturn::SUCCESS,
                SQLSetEnvAttr(
                    env as HEnv,
                    EnvironmentAttribute::OdbcVersion,
                    AttrOdbcVersion::Odbc3.into(),
                    0,
                )
            );

            assert_eq!(
                SqlReturn::SUCCESS,
                SQLSetEnvAttr(
                    env as HEnv,
                    EnvironmentAttribute::ConnectionPooling,
                    AttrConnectionPooling::OnePerHenv.into(),
                    0,
                )
            );
        }

        env as HEnv
    }

    /// Generate the default connection string and returns :
    /// - The connection handle
    /// - The string used as the input connection string
    /// - The retrieved output connection string
    /// - The retrieved length of the output connection string
    fn power_bi_connect(env_handle: HEnv) -> (odbc_sys::HDbc, String, String, SmallInt) {
        // Allocate a DBC handle
        let mut dbc: Handle = null_mut();
        let output_len;
        let in_connection_string;
        let out_connection_string;
        unsafe {
            assert_eq!(
                SqlReturn::SUCCESS,
                SQLAllocHandle(
                    HandleType::Dbc,
                    env_handle as *mut _,
                    &mut dbc as *mut Handle
                )
            );

            // Set the login timeout
            let login_timeout = 15;
            assert_eq!(
                SqlReturn::SUCCESS,
                SQLSetConnectAttrW(
                    dbc as HDbc,
                    ConnectionAttribute::LoginTimeout,
                    login_timeout as Pointer,
                    0,
                )
            );

            // Generate the connection string and add a null terminator because PowerBi uses SQL_NTS for the length
            in_connection_string = generate_default_connection_str();
            let mut in_connection_string_encoded = widechar::to_widechar_vec(&in_connection_string);
            in_connection_string_encoded.push(0);

            let str_len_ptr = &mut 0;
            const BUFFER_LENGTH: SmallInt = 300;
            let mut out_connection_string_buff: [WideChar; BUFFER_LENGTH as usize - 1] =
                [0; (BUFFER_LENGTH as usize - 1)];
            let out_connection_string_buff = &mut out_connection_string_buff as *mut WideChar;

            assert_ne!(
                SqlReturn::ERROR,
                SQLDriverConnectW(
                    dbc as HDbc,
                    null_mut(),
                    in_connection_string_encoded.as_ptr(),
                    SQL_NTS,
                    out_connection_string_buff,
                    BUFFER_LENGTH,
                    str_len_ptr,
                    DriverConnectOption::NoPrompt,
                ),
                "{}",
                get_sql_diagnostics(HandleType::Dbc, dbc)
            );

            output_len = *str_len_ptr;
            out_connection_string = widechar::from_widechar_ref_lossy(slice::from_raw_parts(
                out_connection_string_buff,
                output_len as usize,
            ));
        }
        (
            dbc as HDbc,
            in_connection_string,
            out_connection_string,
            output_len,
        )
    }

    /// Test PowerBI Setup flow
    #[test]
    fn test_setup() {
        setup();
    }

    /// Test PowerBi environment clean-up
    #[test]
    fn test_env_cleanup() {
        // We need a handle to be able to test that freeing the handle work
        let env_handle: HEnv = setup();

        unsafe {
            // Verify that freeing the handle is working as expected
            assert_eq!(
                SqlReturn::SUCCESS,
                SQLFreeHandle(HandleType::Env, env_handle as Handle),
                "{}",
                get_sql_diagnostics(HandleType::Env, env_handle as Handle)
            );
        }
    }

    /// Test PowerBi connection flow (the setup flow is a pre-requisite)
    /// Connection flow is :
    /// - SQLAllocHandle(SQL_HANDLE_DBC)
    /// - SQLSetConnectAttrW(SQL_ATTR_LOGIN_TIMEOUT)
    /// - SQLDriverConnectW({NullTerminatedInConnectionString}, SQL_NTS, {NullTerminatedOutConnectionString}, SQL_NTS, SQL_DRIVER_NOPROMPT)
    /// - SQLGetInfoW(SQL_DRIVER_NAME)
    /// - SQLGetInfoW(SQL_DBMS_NAME)
    /// - SQLGetInfoW(SQL_DBMS_VER)
    #[test]
    fn test_connection() {
        let env_handle: HEnv = setup();
        let (conn_handle, in_connection_string, out_connection_string, output_len) =
            power_bi_connect(env_handle);

        unsafe {
            let input_len = in_connection_string.len() as SmallInt;

            println!("Input connection string = {in_connection_string}\nLength is {input_len}");
            println!("Output connection string = {out_connection_string}\nLength is {output_len}");
            // The output string should be the same as the input string except with extra curly braces around the driver name
            assert_eq!(input_len, output_len, "Expect that both connection the input connection string and output connection string have the same length but input string length is {input_len} and output string length is {output_len}");

            // SQL_DRIVER_NAME is not accessible through odbc_sys
            /*
            assert_eq!(
                SqlReturn::SUCCESS,
                SQLGetInfoW(
                    dbc as HDbc,
                    SQL_DRIVER_NAME, // 6
                    driver_name as Pointer,
                    BUFFER_LENGTH,
                    str_len_ptr
                )
            );
             */

            test_get_info!(conn_handle, InfoType::DbmsName, 28, DataType::WChar);

            test_get_info!(conn_handle, InfoType::DbmsVer, 58, DataType::WChar);
        }
    }

    // Test PowerBI driver information retrieval
    // This test is limited by the available InfoType values in odbc_sys
    #[test]
    fn test_get_driver_info() {
        let env_handle: HEnv = setup();
        let (conn_handle, _, _, _) = power_bi_connect(env_handle);

        unsafe {
            test_get_info!(
                conn_handle,
                InfoType::IdentifierQuoteChar,
                4,
                DataType::WChar
            );
            // SQL-1177: Investigate how to test missing InfoType values
            // InfoType::SQL_OWNER_USAGE
            // InfoType::SQL_CATALOG_USAGE
            // InfoType::SQL_CATALOG_NAME_SEPARATOR
            // InfoType::SQL_CATALOG_LOCATION
            // InfoType::SQL_SQL_CONFORMANCE
            test_get_info!(
                conn_handle,
                InfoType::MaxColumnsInOrderBy,
                2,
                DataType::USmallInt
            );
            test_get_info!(
                conn_handle,
                InfoType::MaxIdentifierLen,
                2,
                DataType::USmallInt
            );
            test_get_info!(
                conn_handle,
                InfoType::MaxColumnsInGroupBy,
                2,
                DataType::USmallInt
            );
            test_get_info!(
                conn_handle,
                InfoType::MaxColumnsInSelect,
                2,
                DataType::USmallInt
            );
            test_get_info!(
                conn_handle,
                InfoType::OrderByColumnsInSelect,
                4,
                DataType::WChar
            );
            // InfoType::SQL_STRING_FUNCTIONS
            // InfoType::SQL_AGGREGATE_FUNCTIONS
            // InfoType::SQL_SQL92_PREDICATES
            // InfoType::SQL_SQL92_RELATIONAL_JOIN_OPERATORS
            // InfoType::SQL_COLUMN_ALIAS
            // InfoType::SQL_GROUP_BY
            // InfoType::SQL_NUMERIC_FUNCTIONS
            // InfoType::SQL_TIMEDATE_FUNCTIONS
            // InfoType::SQL_SYSTEM_FUNCTIONS
            // InfoType::SQL_TIMEDATE_ADD_INTERVALS
            // InfoType::SQL_TIMEDATE_DIFF_INTERVALS
            // InfoType::SQL_CONCAT_NULL_BEHAVIOR
            test_get_info!(conn_handle, InfoType::CatalogName, 4, DataType::WChar);
            // InfoType::SQL_CATALOG_TERM
            // InfoType::SQL_OWNER_TERM
            // InfoType::SQL_ODBC_INTERFACE_CONFORMANCE
            test_get_info!(
                conn_handle,
                InfoType::SearchPatternEscape,
                2,
                DataType::WChar
            );
            // InfoType::SQL_CONVERT_FUNCTIONS
            // InfoType::SQL_CONVERT_BIGINT
            // InfoType::SQL_CONVERT_BINARY
            // InfoType::SQL_CONVERT_BIT
            // InfoType::SQL_CONVERT_CHAR
            // InfoType::SQL_CONVERT_DECIMAL
            // InfoType::SQL_CONVERT_DOUBLE
            // InfoType::SQL_CONVERT_FLOAT
            // InfoType::SQL_CONVERT_GUID
            // InfoType::SQL_CONVERT_INTEGER
            // InfoType::SQL_CONVERT_LONGVARBINARY
            // InfoType::SQL_CONVERT_LONGVARCHAR
            // InfoType::SQL_CONVERT_NUMERIC
            // InfoType::SQL_CONVERT_REAL
            // InfoType::SQL_CONVERT_SMALLINT
            // InfoType::SQL_CONVERT_TIMESTAMP
            // InfoType::SQL_CONVERT_TINYINT
            // InfoType::SQL_CONVERT_DATE
            // InfoType::SQL_CONVERT_TIME
            // InfoType::SQL_CONVERT_VARBINARY
            // InfoType::SQL_CONVERT_VARCHAR
            // InfoType::SQL_CONVERT_WCHAR
            // InfoType::SQL_CONVERT_WLONGVARCHAR
            // InfoType::SQL_CONVERT_WVARCHAR
            test_get_info!(
                conn_handle,
                InfoType::SpecialCharacters,
                44,
                DataType::WChar
            );
            // InfoType::SQL_RETURN_ESCAPE_CLAUSE
            // InfoType::SQL_DRIVER_ODBC_VER
        }
    }
}