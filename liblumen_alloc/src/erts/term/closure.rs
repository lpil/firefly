use core::alloc::Layout;
use core::cmp::Ordering;
use core::convert::TryFrom;
use core::ffi::c_void;
use core::fmt::{self, Debug, Display, Write};
use core::hash::{Hash, Hasher};
use core::mem::{self, size_of};
use core::ptr::{self, NonNull};
use core::slice;

use crate::borrow::CloneToProcess;
use crate::erts::apply::DynamicCallee;
use crate::erts::exception::AllocResult;
use crate::erts::process::alloc::{Heap, TermAlloc};
use crate::erts::process::{Frame, FrameWithArguments, Native};
use crate::erts::{self, to_word_size, Arity, ModuleFunctionArity};

use super::prelude::*;

#[export_name = "lumen_closure_size"]
pub extern "C" fn closure_size(pointer_width: u32, env_len: u32) -> u32 {
    if pointer_width == 64 {
        let layout = ClosureLayout::layout_64bit(env_len as usize);
        layout.size() as u32
    } else {
        assert_eq!(pointer_width, 32);
        let layout = ClosureLayout::layout_32bit(env_len as usize);
        layout.size() as u32
    }
}

#[repr(C)]
pub struct Closure {
    header: Header<Closure>,
    module: Atom,
    arity: u32,
    definition: Definition,
    /// Pointer to function entry.  When a closure is received over ETF, this may be `None`.
    native: Option<NonNull<c_void>>,
    env: [Term],
}
impl_dynamic_header!(Closure, Term::HEADER_CLOSURE);

#[derive(Debug, Clone, Copy)]
pub struct ClosureLayout {
    layout: Layout,
    module_offset: usize,
    arity_offset: usize,
    definition_offset: usize,
    native_offset: usize,
    env_offset: usize,
}
impl ClosureLayout {
    #[inline]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    #[inline(always)]
    pub fn base_size() -> usize {
        if cfg!(target_pointer_width = "64") {
            debug_assert_eq!(Self::base_size_native(), Self::base_size_64bit());
            Self::base_size_64bit()
        } else {
            assert!(cfg!(target_pointer_width = "32"));
            debug_assert_eq!(Self::base_size_native(), Self::base_size_32bit());
            Self::base_size_32bit()
        }
    }

    pub fn base_layout_32bit() -> Layout {
        // Header<T> + Atom
        let (layout, _module_offset) = Layout::new::<u32>().extend(Layout::new::<u32>()).unwrap();
        // u32
        let (layout, _arity_offset) = layout.extend(Layout::new::<u32>()).unwrap();
        // Definition
        let (layout, _definition_offset) = layout.extend(Layout::new::<Definition32>()).unwrap();
        // Option<NonNull<c_void>>
        let (layout, _native_offset) = layout
            .extend(Layout::new::<Option<NonNull<c_void>>>())
            .unwrap();
        layout
    }

    pub fn layout_32bit(env_len: usize) -> Layout {
        let layout = Self::base_layout_32bit();
        let env = unsafe {
            let ptr = ptr::null_mut() as *mut u64;
            let arr = core::slice::from_raw_parts(ptr as *const (), env_len);
            &*(arr as *const [()] as *mut [u64])
        };
        let (layout, _env_offset) = layout.extend(Layout::for_value(env)).unwrap();
        layout.pad_to_align()
    }

    pub fn base_size_32bit() -> usize {
        let layout = Self::base_layout_32bit();
        layout.size()
    }

    pub fn base_layout_64bit() -> Layout {
        // Header<T> + Atom
        let (layout, _module_offset) = Layout::new::<u64>().extend(Layout::new::<u64>()).unwrap();
        // u32
        let (layout, _arity_offset) = layout.extend(Layout::new::<u32>()).unwrap();
        // Definition
        let (layout, _definition_offset) = layout.extend(Layout::new::<Definition64>()).unwrap();
        // Option<NonNull<c_void>>
        let (layout, _native_offset) = layout
            .extend(Layout::new::<Option<NonNull<c_void>>>())
            .unwrap();
        layout
    }

    pub fn layout_64bit(env_len: usize) -> Layout {
        let layout = Self::base_layout_64bit();
        let env = unsafe {
            let ptr = ptr::null_mut() as *mut u32;
            let arr = core::slice::from_raw_parts(ptr as *const (), env_len);
            &*(arr as *const [()] as *mut [u32])
        };
        let (layout, _env_offset) = layout.extend(Layout::for_value(env)).unwrap();
        layout.pad_to_align()
    }

    pub fn base_size_64bit() -> usize {
        let layout = Self::base_layout_64bit();
        layout.size()
    }

    fn base_size_native() -> usize {
        let (layout, _module_offset) = Layout::new::<Header<Closure>>()
            .extend(Layout::new::<Atom>())
            .unwrap();
        let (layout, _arity_offset) = layout.extend(Layout::new::<u32>()).unwrap();
        let (layout, _definition_offset) = layout.extend(Layout::new::<Definition>()).unwrap();
        let (layout, _native_offset) = layout
            .extend(Layout::new::<Option<NonNull<c_void>>>())
            .unwrap();
        layout.size()
    }

    pub fn for_env(env: &[Term]) -> Self {
        let (layout, module_offset) = Layout::new::<Header<Closure>>()
            .extend(Layout::new::<Atom>())
            .unwrap();
        let (layout, arity_offset) = layout.extend(Layout::new::<u32>()).unwrap();
        let (layout, definition_offset) = layout.extend(Layout::new::<Definition>()).unwrap();
        let (layout, native_offset) = layout
            .extend(Layout::new::<Option<NonNull<c_void>>>())
            .unwrap();
        let (layout, env_offset) = layout.extend(Layout::for_value(env)).unwrap();

        let layout = layout.pad_to_align();

        Self {
            layout,
            module_offset,
            definition_offset,
            arity_offset,
            native_offset,
            env_offset,
        }
    }

    pub fn for_env_len(env_len: usize) -> Self {
        unsafe {
            let ptr = ptr::null_mut() as *mut Term;
            let arr = core::slice::from_raw_parts(ptr as *const (), env_len);
            let env = &*(arr as *const [()] as *mut [Term]);
            Self::for_env(env)
        }
    }
}
impl Closure {
    /// Constructs a new `Closure` with an anonymous definition, with an env of size `len` using
    /// `heap`
    ///
    /// The constructed closure will contain an environment of invalid words until
    /// individual elements are written, this is intended for cases where we don't
    /// already have a slice of elements to construct a tuple from
    pub fn new_anonymous<A>(
        heap: &mut A,
        module: Atom,
        index: Index,
        old_unique: OldUnique,
        unique: Unique,
        arity: Arity,
        native: Option<NonNull<c_void>>,
        _creator: Creator,
        env_len: usize,
    ) -> AllocResult<Boxed<Self>>
    where
        A: ?Sized + Heap,
    {
        let definition = Definition::Anonymous {
            index: index as usize,
            unique,
            old_unique,
        };
        Self::new(heap, module, definition, arity, native, env_len)
    }

    /// Like `new_anonymous`, but for export definitions
    pub fn new_export<A>(
        heap: &mut A,
        module: Atom,
        function: Atom,
        arity: Arity,
        native: Option<NonNull<c_void>>,
    ) -> AllocResult<Boxed<Self>>
    where
        A: ?Sized + Heap,
    {
        let definition = Definition::Export { function };
        Self::new(heap, module, definition, arity, native, 0)
    }

    /// Internal helper for the `new_*` constructors
    fn new<A>(
        heap: &mut A,
        module: Atom,
        definition: Definition,
        arity: Arity,
        native: Option<NonNull<c_void>>,
        env_len: usize,
    ) -> AllocResult<Boxed<Self>>
    where
        A: ?Sized + Heap,
    {
        let closure_layout = ClosureLayout::for_env_len(env_len);
        let layout = closure_layout.layout.clone();

        let header_arity = to_word_size(layout.size() - size_of::<Header<Closure>>());
        let header = Header::from_arity(header_arity);
        unsafe {
            // Allocate space for closure
            let ptr = heap.alloc_layout(layout)?.as_ptr() as *mut u8;
            let header_ptr = ptr as *mut Header<Self>;
            // Write header
            header_ptr.write(header);
            // Construct pointer to each field and write the corresponding value
            let module_ptr = ptr.offset(closure_layout.module_offset as isize) as *mut Atom;
            module_ptr.write(module);
            let arity_ptr = ptr.offset(closure_layout.arity_offset as isize) as *mut u32;
            arity_ptr.write(arity.into());
            let definition_ptr =
                ptr.offset(closure_layout.definition_offset as isize) as *mut Definition;
            definition_ptr.write(definition);
            let native_ptr =
                ptr.offset(closure_layout.native_offset as isize) as *mut Option<NonNull<c_void>>;
            native_ptr.write(native);
            // Construct actual Closure reference
            Ok(Self::from_raw_parts::<Term>(ptr as *mut Term, env_len))
        }
    }

    pub fn from_slice<A>(
        heap: &mut A,
        module: Atom,
        index: Index,
        old_unique: OldUnique,
        unique: Unique,
        arity: Arity,
        native: Option<NonNull<c_void>>,
        _creator: Creator,
        env: &[Term],
    ) -> AllocResult<Boxed<Self>>
    where
        A: ?Sized + TermAlloc,
    {
        let definition = Definition::Anonymous {
            index: index as usize,
            unique,
            old_unique,
        };

        Self::new_from_slice(heap, module, definition, arity, native, env)
    }

    fn new_from_slice<A>(
        heap: &mut A,
        module: Atom,
        definition: Definition,
        arity: Arity,
        native: Option<NonNull<c_void>>,
        env: &[Term],
    ) -> AllocResult<Boxed<Self>>
    where
        A: ?Sized + TermAlloc,
    {
        let closure_layout = ClosureLayout::for_env(env);
        let layout = closure_layout.layout.clone();

        // The result of calling this will be a Closure with everything located
        // contiguously in memory
        let header_arity = to_word_size(layout.size() - size_of::<Header<Closure>>());
        let header = Header::from_arity(header_arity);
        unsafe {
            // Allocate space for tuple and immediate elements
            let ptr = heap.alloc_layout(layout)?.as_ptr() as *mut u8;
            // Write tuple header
            let header_ptr = ptr as *mut Header<Self>;
            header_ptr.write(header);
            // Construct pointer to each field and write the corresponding value
            let module_ptr = ptr.offset(closure_layout.module_offset as isize) as *mut Atom;
            module_ptr.write(module);
            let arity_ptr = ptr.offset(closure_layout.arity_offset as isize) as *mut u32;
            arity_ptr.write(arity.into());
            let definition_ptr =
                ptr.offset(closure_layout.definition_offset as isize) as *mut Definition;
            definition_ptr.write(definition);
            let native_ptr =
                ptr.offset(closure_layout.native_offset as isize) as *mut Option<NonNull<c_void>>;
            native_ptr.write(native);
            // Construct pointer to first env element
            let mut env_ptr = ptr.offset(closure_layout.env_offset as isize) as *mut Term;
            // Walk original slice of terms and copy them into new memory region,
            // copying boxed terms recursively as necessary
            for element in env {
                if element.is_immediate() {
                    env_ptr.write(*element);
                } else {
                    // Recursively call clone_to_heap, and then write the box header here
                    let boxed = element.clone_to_heap(heap)?;
                    env_ptr.write(boxed);
                }

                env_ptr = env_ptr.offset(1);
            }
            // Construct actual Closure reference
            Ok(Self::from_raw_parts::<Term>(ptr as *mut Term, env.len()))
        }
    }

    pub fn clone_to<A>(&self, heap: &mut A) -> AllocResult<Boxed<Closure>>
    where
        A: ?Sized + TermAlloc,
    {
        let module = self.module.clone();
        let definition = self.definition.clone();
        let arity = self.arity;
        let native = self.native.clone();
        Self::new_from_slice(heap, module, definition, arity as u8, native, &self.env)
    }

    #[inline]
    pub fn base_size_words() -> usize {
        erts::to_word_size(ClosureLayout::base_size())
    }

    #[inline]
    pub fn arity(&self) -> Arity {
        self.arity as Arity
    }

    #[inline]
    pub fn definition(&self) -> &Definition {
        &self.definition
    }

    #[inline]
    pub fn callee(&self) -> Option<DynamicCallee> {
        self.native
            .map(|nn| unsafe { mem::transmute::<*const c_void, DynamicCallee>(nn.as_ptr()) })
    }

    pub fn native(&self) -> NonNull<c_void> {
        self.native.unwrap_or_else(|| {
            panic!(
                "{} does not have a native function associated with it",
                self.module_function_arity()
            )
        })
    }

    /// The `native` function needs takes the following arguments
    ///
    /// If `env_len() == 0`, then this `Closure` is not passed
    /// 0.. - Any explicit arguments for its `arity`.
    ///
    /// If the `env_len() > 0`, then this `Closure` is passed
    /// 0 - Closure `Term`, so native code can extract the `env_slice()`
    /// 1.. - Any explicit arguments for its `arity`.
    pub fn native_arity(&self) -> Arity {
        let closure_term_arity = if self.env_len() > 0 { 1 } else { 0 };

        (closure_term_arity + self.arity) as Arity
    }

    pub fn frame(&self) -> Frame {
        Frame::from_definition(
            self.module,
            self.definition.clone(),
            self.arity as u8,
            unsafe { Native::from_ptr(self.native().as_ptr(), self.native_arity()) },
        )
    }

    pub fn module_function_arity(&self) -> ModuleFunctionArity {
        ModuleFunctionArity {
            module: self.module,
            function: self.function(),
            arity: self.arity as u8,
        }
    }

    pub fn frame_with_arguments(
        &self,
        uses_returned: bool,
        arguments: Vec<Term>,
    ) -> FrameWithArguments {
        let mut full_arguments = Vec::with_capacity(self.native_arity() as usize);

        if self.env_len() > 0 {
            full_arguments.push(self.encode().unwrap());
        }

        full_arguments.extend_from_slice(&arguments);

        self.frame().with_arguments(uses_returned, &full_arguments)
    }

    #[inline]
    pub fn module(&self) -> Atom {
        self.module.clone()
    }

    #[inline]
    pub fn function(&self) -> Atom {
        self.definition.function_name()
    }

    #[inline]
    pub fn native_address(&self) -> Option<usize> {
        self.native.map(|nn| nn.as_ptr() as usize)
    }

    /// Returns the length of the closure environment in terms.
    #[inline]
    pub fn env_len(&self) -> usize {
        self.env.len()
    }

    /// Returns a slice containing the closure environment.
    #[inline]
    pub fn env_slice(&self) -> &[Term] {
        &self.env
    }

    /// Returns a mutable slice containing the closure environment.
    #[inline]
    pub fn env_slice_mut(&mut self) -> &mut [Term] {
        &mut self.env
    }

    /// Iterator over the terms in the closure environment.
    #[inline]
    pub fn env_iter<'a>(&'a self) -> slice::Iter<'a, Term> {
        self.env.iter()
    }

    /// Gets an element from the environment directly by index.
    /// Panics if outside of bounds.
    #[inline]
    pub fn get_env_element(&self, idx: usize) -> Term {
        self.env[idx]
    }

    /// Given a raw pointer to some memory, and a length in units of `Self::Element`,
    /// this function produces a fat pointer to `Self` which represents a value
    /// containing `len` elements in its variable-length field
    ///
    /// For example, given a pointer to the memory containing a `Closure`, and the
    /// number of elements it contains, this function produces a valid pointer of
    /// type `Closure`
    unsafe fn from_raw_parts<E: super::arch::Repr>(ptr: *const E, len: usize) -> Boxed<Closure> {
        // Invariants of slice::from_raw_parts.
        assert!(!ptr.is_null());
        assert!(
            len <= isize::max_value() as usize,
            "len ({}) is not less than isize::max_value() ({})",
            len,
            isize::max_value() as usize
        );

        let slice = core::slice::from_raw_parts_mut(ptr as *mut E, len);
        let ptr = slice as *mut [E] as *mut Closure;
        Boxed::new_unchecked(ptr)
    }
}

impl<E: crate::erts::term::arch::Repr> Boxable<E> for Closure {}

impl<E: super::arch::Repr> UnsizedBoxable<E> for Closure {
    unsafe fn from_raw_term(ptr: *mut E) -> Boxed<Closure> {
        let header = &*(ptr as *mut Header<Closure>);
        let arity = header.arity();
        // -1 is size of header in words
        let env_len = arity - (Self::base_size_words() - 1);

        Self::from_raw_parts::<E>(ptr, env_len)
    }
}

impl CloneToProcess for Closure {
    #[inline]
    fn clone_to_heap<A>(&self, heap: &mut A) -> AllocResult<Term>
    where
        A: ?Sized + TermAlloc,
    {
        self.clone_to(heap).map(|boxed| boxed.into())
    }

    fn size_in_words(&self) -> usize {
        let mut size = erts::to_word_size(Layout::for_value(self).size());
        for element in &self.env {
            size += element.size_in_words()
        }
        size
    }
}

impl Debug for Closure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Closure")
            .field("header", &self.header)
            .field("module", &self.module)
            .field("arity", &self.arity)
            .field("definition", &self.definition)
            .field("native", &self.native.map(|nn| nn.as_ptr()))
            .field("env_len", &self.env.len())
            .field("env", &self.env.iter().copied().collect::<Vec<Term>>())
            .finish()
    }
}

impl Display for Closure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let module_function_arity = &self.module_function_arity();

        write!(f, "&{}.", module_function_arity.module)?;
        f.write_char('\"')?;
        module_function_arity
            .function
            .name()
            .chars()
            .flat_map(char::escape_default)
            .try_for_each(|c| f.write_char(c))?;
        f.write_char('\"')?;
        write!(f, "/{}", module_function_arity.arity)
    }
}

impl Eq for Closure {}

impl Hash for Closure {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.module.hash(state);
        self.arity.hash(state);
        self.definition.hash(state);
        self.native_address().hash(state);
        self.env_slice().hash(state);
    }
}

impl Ord for Closure {
    fn cmp(&self, other: &Self) -> Ordering {
        self.module
            .cmp(&other.module)
            .then_with(|| self.arity.cmp(&other.arity))
            .then_with(|| self.definition.cmp(&other.definition))
            .then_with(|| self.native_address().cmp(&other.native_address()))
            .then_with(|| self.env_slice().cmp(other.env_slice()))
    }
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        (self.module == other.module)
            && (self.arity == other.arity)
            && (self.definition == other.definition)
            && (self.native_address() == other.native_address())
            && (self.env_slice() == other.env_slice())
    }
}
impl<T> PartialEq<Boxed<T>> for Closure
where
    T: ?Sized + PartialEq<Closure>,
{
    #[inline]
    fn eq(&self, other: &Boxed<T>) -> bool {
        other.as_ref().eq(self)
    }
}

impl PartialOrd for Closure {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> PartialOrd<Boxed<T>> for Closure
where
    T: ?Sized + PartialOrd<Closure>,
{
    #[inline]
    fn partial_cmp(&self, other: &Boxed<T>) -> Option<Ordering> {
        other.as_ref().partial_cmp(self).map(|o| o.reverse())
    }
}

impl TryFrom<TypedTerm> for Boxed<Closure> {
    type Error = TypeError;

    fn try_from(typed_term: TypedTerm) -> Result<Self, Self::Error> {
        match typed_term {
            TypedTerm::Closure(closure) => Ok(closure),
            _ => Err(TypeError),
        }
    }
}

#[derive(Clone)]
pub enum Creator {
    Local(Pid),
    External(ExternalPid),
}

impl Debug for Creator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Local(pid) => write!(f, "{:?}", pid),
            Self::External(external_pid) => write!(f, "{:?}", external_pid),
        }
    }
}

impl From<Pid> for Creator {
    fn from(pid: Pid) -> Self {
        Self::Local(pid)
    }
}

impl From<ExternalPid> for Creator {
    fn from(external_pid: ExternalPid) -> Self {
        Self::External(external_pid)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub enum Definition {
    /// External functions captured with `fun M:F/A` in Erlang or `&M.f/a` in Elixir.
    Export { function: Atom },
    /// Anonymous functions declared with `fun` in Erlang or `fn` in Elixir.
    Anonymous {
        /// Each anonymous function within a module has an unique index.
        index: usize,
        /// The 16 bytes MD5 of the significant parts of the Beam file.
        unique: [u8; 16],
        /// The hash value of the parse tree for the fun, but must fit in i32, so not the same as
        /// `unique`.
        old_unique: u32,
        /* Not used in Lumen, needed for term_to_binary/external communication
         * creator: Creator, */
    },
}

// For size calculations
#[repr(C)]
#[allow(unused)]
enum Definition32 {
    /// External functions captured with `fun M:F/A` in Erlang or `&M.f/a` in Elixir.
    Export { function: u32 },
    /// Anonymous functions declared with `fun` in Erlang or `fn` in Elixir.
    Anonymous {
        /// Each anonymous function within a module has an unique index.
        index: u32,
        /// The 16 bytes MD5 of the significant parts of the Beam file.
        unique: [u8; 16],
        /// The hash value of the parse tree for the fun, but must fit in i32, so not the same as
        /// `unique`.
        old_unique: u32,
        /* Not used in Lumen, needed for term_to_binary/external communication
         * creator: Creator, */
    },
}

// For size calculations
#[repr(C)]
#[allow(unused)]
enum Definition64 {
    /// External functions captured with `fun M:F/A` in Erlang or `&M.f/a` in Elixir.
    Export { function: u64 },
    /// Anonymous functions declared with `fun` in Erlang or `fn` in Elixir.
    Anonymous {
        /// Each anonymous function within a module has an unique index.
        index: u64,
        /// The 16 bytes MD5 of the significant parts of the Beam file.
        unique: [u8; 16],
        /// The hash value of the parse tree for the fun, but must fit in i32, so not the same as
        /// `unique`.
        old_unique: u32,
        /* Not used in Lumen, needed for term_to_binary/external communication
         * creator: Creator, */
    },
}

impl Definition {
    pub fn function_name(&self) -> Atom {
        match self {
            Definition::Export { function } => *function,
            Definition::Anonymous {
                index,
                old_unique,
                unique,
                ..
            } => Atom::from_str(format!(
                "{}-{}-{}",
                index,
                old_unique,
                Self::format_unique(&unique)
            )),
        }
    }

    fn format_unique(unique: &[u8; 16]) -> String {
        let mut string = String::with_capacity(unique.len() * 2);

        for byte in unique {
            string.push_str(&format!("{:02x}", byte));
        }

        string
    }
}

impl Eq for Definition {}

impl Hash for Definition {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Definition::Export { function } => function.hash(state),
            Definition::Anonymous {
                index,
                unique,
                old_unique,
                ..
            } => {
                index.hash(state);
                unique.hash(state);
                old_unique.hash(state);
            }
        }
    }
}

impl PartialEq for Definition {
    fn eq(&self, other: &Definition) -> bool {
        match (self, other) {
            (
                Definition::Export {
                    function: self_function,
                },
                Definition::Export {
                    function: other_function,
                },
            ) => self_function == other_function,
            (
                Definition::Anonymous {
                    index: self_index,
                    unique: self_unique,
                    old_unique: self_old_unique,
                    ..
                },
                Definition::Anonymous {
                    index: other_index,
                    unique: other_unique,
                    old_unique: other_old_unique,
                    ..
                },
            ) => {
                (self_index == other_index)
                    && (self_unique == other_unique)
                    && (self_old_unique == other_old_unique)
            }
            _ => false,
        }
    }
}

impl PartialOrd for Definition {
    fn partial_cmp(&self, other: &Definition) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Definition {
    fn cmp(&self, other: &Definition) -> Ordering {
        match (self, other) {
            (
                Definition::Export {
                    function: self_function,
                },
                Definition::Export {
                    function: other_function,
                },
            ) => self_function.cmp(other_function),
            (Definition::Export { .. }, Definition::Anonymous { .. }) => Ordering::Greater,
            (Definition::Anonymous { .. }, Definition::Export { .. }) => Ordering::Less,
            (
                Definition::Anonymous {
                    index: self_index,
                    unique: self_unique,
                    old_unique: self_old_unique,
                    ..
                },
                Definition::Anonymous {
                    index: other_index,
                    unique: other_unique,
                    old_unique: other_old_unique,
                    ..
                },
            ) => self_index
                .cmp(other_index)
                .then_with(|| self_unique.cmp(other_unique))
                .then_with(|| self_old_unique.cmp(other_old_unique)),
        }
    }
}

/// Index of anonymous function in module's function table
pub type Index = u32;

/// Hash of the parse of the function.  Replaced by `Unique`
pub type OldUnique = u32;

/// 16 byte MD5 of the significant parts of the BEAM file.
pub type Unique = [u8; 16];
