//! ★ ประตูเดียวของข้อความที่ผู้ใช้เห็น — ห้ามเขียนสตริงตรงใน widget
//!
//! spec: docs/03-modes-and-ui.md §0 (ตัดสิน 28 ก.ค. 2026)
//!
//! กลุ่มเป้าหมายคือนักวาดหลายประเทศ **UI หลักเป็นอังกฤษ ไทยเป็นภาษาที่สอง**
//! ยังไม่รองรับ CJK (ฟอนต์ครบชุด 16+ MB ทะลุเพดาน binary 25 MB — docs/08 §6)
//!
//! ตอนนี้ข้างในเป็นแค่ `match` ธรรมดา **ยังไม่เพิ่ม dependency** ประเด็นคือ
//! **มีช่องต่อ** — พอถึงเวลาทำระบบเต็มรูปแบบจะเป็นงานเชิงกล ไม่ใช่การรื้อทั้งโปรแกรม
//!
//! ### กฎที่บังคับด้วยชนิดข้อมูล ไม่ใช่ด้วยวินัย
//!
//! [`th`] คืน `Option<&str>` → คำแปลที่ยังไม่มี **ตกกลับเป็นอังกฤษเอง**
//! เป็นไปไม่ได้เลยที่ผู้ใช้จะเห็น key หรือช่องว่าง เพราะไม่มีทางเขียนโค้ดแบบนั้นได้
//!
//! ### ข้อความ error
//!
//! `#[error(…)]` ของ `thiserror` เป็น format string ตอนคอมไพล์ → แปลตอนรันไม่ได้
//! จึงแยกบทบาท: `#[error(…)]` เป็น **อังกฤษสำหรับ log/นักพัฒนา** (ค้นหาง่าย
//! ผู้ใช้ส่ง log มาให้เราอ่านได้) ส่วนข้อความที่ผู้ใช้เห็นประกอบขึ้นที่นี่
//! **จากฟิลด์ของ error** แล้วแปลตามภาษา (ดู [`load_error`], [`job_failure`])

use refx_asset::decode::LoadError;
use refx_asset::pool::JobFailure;
use refx_core::clipboard::ClipboardError;
use refx_render::atlas::AtlasError;

/// ภาษาของ UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// อังกฤษ — ภาษาหลัก และเป็นตัวสำรองเสมอเมื่อคำแปลไม่ครบ
    #[default]
    En,
    /// ไทย
    Th,
}

impl Lang {
    /// เลือกภาษาจากแท็กของ OS เช่น `"th-TH"` `"en_US.UTF-8"`
    ///
    /// ไม่รู้จัก → [`Lang::En`] (docs/03 §0 ข้อ 3)
    #[must_use]
    pub fn from_tag(tag: &str) -> Self {
        match refx_platform::locale::primary_language(tag).as_str() {
            "th" => Self::Th,
            _ => Self::En,
        }
    }

    /// ภาษาที่ควรใช้บนเครื่องนี้ — อ่านจาก OS ครั้งเดียวตอนเปิดโปรแกรม
    #[must_use]
    pub fn from_system() -> Self {
        let lang = refx_platform::locale::user_language_tag()
            .as_deref()
            .map_or(Self::En, Self::from_tag);
        tracing::info!(?lang, "UI language selected");
        lang
    }
}

/// รหัสของข้อความคงที่ทุกตัวที่ผู้ใช้เห็น
///
/// เพิ่มรายการใหม่ที่นี่เท่านั้น — คอมไพเลอร์จะบังคับให้ไปเติมข้อความอังกฤษให้ครบเอง
/// (`match` ใน [`en`] ไม่มี `_ =>` โดยตั้งใจ)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// สถานะเริ่มต้นบน status bar
    Ready,
    /// เปิด cache ไม่ได้ แต่โปรแกรมยังใช้งานได้
    RunningWithoutCache,
    /// ชื่อ board ที่ยังไม่ได้ตั้งชื่อ
    UntitledBoard,
    /// tooltip ของปุ่มเปิด board ใหม่
    NewBoardHint,
    /// หัวข้อ panel ซ้าย
    Library,
    /// คำอธิบายในช่อง library ที่ยังว่าง
    LibraryPlaceholder,
    /// คำใบ้ว่าเอาภาพเข้ามาได้ยังไง (ลากไฟล์ หรือ Ctrl+V)
    LibraryDropHint,
    /// หัวข้อ panel ขวา
    Inspector,
    /// รายการคุณสมบัติของ Canvas mode
    InspectorCanvasGeometry,
    /// รายการคุณสมบัติของ Canvas mode (ต่อ)
    InspectorCanvasTransform,
    /// รายการคุณสมบัติของ Arrange mode
    InspectorArrangeMeta,
    /// รายการคุณสมบัติของ Arrange mode (ต่อ)
    InspectorArrangeGroup,
    /// เครื่องมือ: เลือก
    ToolSelect,
    /// เครื่องมือ: ย้าย
    ToolMove,
    /// เครื่องมือ: ครอป
    ToolCrop,
    /// เครื่องมือ: จิ้มสี
    ToolPicker,
    /// เครื่องมือ: ไม้บรรทัด
    ToolMeasure,
    /// เครื่องมือ: โน้ตข้อความ
    ToolText,
    /// เครื่องมือ: ขาวดำ
    ToolGrayscale,
    /// เครื่องมือ: เรียง
    ToolSort,
    /// เครื่องมือ: กรอง
    ToolFilter,
    /// เครื่องมือ: ติดแท็ก
    ToolTag,
    /// เครื่องมือ: ส่งภาพเข้า canvas
    ToolSendToCanvas,
    /// กำลังอ่าน clipboard หลังผู้ใช้กด Ctrl+V (P1-8)
    ReadingClipboard,
    /// กด Ctrl+Z แล้วไม่มีอะไรให้ย้อน
    NothingToUndo,
    /// กด Ctrl+Y แล้วไม่มีอะไรให้ทำซ้ำ
    NothingToRedo,
    /// กด Delete แต่ไม่มีอะไรที่ลบได้ (ไม่ได้เลือก หรือเลือกแต่ภาพที่ล็อกไว้)
    NothingToDelete,
    /// เลือกภาพก่อนถึงจะปรับได้
    InspectorNoSelection,
    /// ปุ่ม "หาไฟล์เอง" ของภาพที่หาย (P4-6 ขั้นที่ 5)
    FindFile,
    /// กำลังรอผู้ใช้ชี้ไฟล์ภาพที่หาย (P4-6 ขั้นที่ 5)
    FindFileChoosing,
    /// ความทึบ
    Opacity,
    /// กลับสี
    Invert,
    /// ความสว่าง
    Brightness,
    /// คอนทราสต์
    Contrast,
    /// พลิกภาพ
    Flip,
    /// ชิดซ้าย
    AlignLeft,
    /// กึ่งกลางแนวนอน
    AlignCentreX,
    /// ชิดขวา
    AlignRight,
    /// ชิดบน
    AlignTop,
    /// กึ่งกลางแนวตั้ง
    AlignCentreY,
    /// ชิดล่าง
    AlignBottom,
    /// กระจายแนวนอน
    DistributeX,
    /// กระจายแนวตั้ง
    DistributeY,
    /// เลือกอย่างน้อยสองภาพก่อนถึงจะจัดเรียงได้
    NothingToArrange,
    /// กำลังอ่านสีจากไฟล์ต้นฉบับ (P2-10)
    ReadingColour,
    /// จิ้มโดนที่ว่าง ไม่มีภาพให้อ่านสี
    NothingToPick,
    /// อ่านสีจากไฟล์ต้นฉบับไม่ได้
    ColourUnavailable,
    /// หัวข้อช่องแก้โน้ต
    Note,
    /// ข้อความจาง ๆ ในช่องโน้ตที่ยังว่าง
    NoteHint,
    /// หัวข้อดาว
    Rating,
    /// หัวข้อป้ายสี
    ColorLabelTitle,
    /// ไม่มีป้ายสี
    ColorLabelNone,
    /// ป้ายสีที่รุ่นนี้ไม่รู้จัก
    ColorLabelUnknown,
    /// ปักหมุด
    Pinned,
    /// หัวข้อแท็ก
    Tags,
    /// คำอธิบายช่องเพิ่มแท็ก
    AddTagHint,
    /// คำอธิบายการถอดแท็ก
    RemoveTagHint,
    /// โน้ตของ metadata
    MetaNote,

    // ---- P3-4: sort + filter ของ Arrange ----
    /// เรียงตามเวลาที่เพิ่ม
    SortAddedAt,
    /// เรียงตามชื่อไฟล์
    SortName,
    /// เรียงตามดาว
    SortRating,
    /// เรียงตามป้ายสี
    SortColorLabel,
    /// เรียงตามสัดส่วนภาพ
    SortAspect,
    /// เรียงตามวันที่แก้ไขไฟล์
    SortModifiedAt,
    /// เรียงตามขนาดไฟล์
    SortFileSize,
    /// เรียงตามตำแหน่งบน canvas (P3-6)
    SortCanvasOrder,
    /// น้อยไปมาก
    SortAscending,
    /// มากไปน้อย
    SortDescending,
    /// ไม่กรอง (ตัวเลือกในรายการ)
    FilterAny,
    /// ข้อความจาง ๆ ในช่องคำค้น
    FilterSearchHint,
    /// ล้างตัวกรองทั้งหมด
    FilterClear,
    /// ดาวขั้นต่ำ
    FilterMinRating,
    /// เฉพาะที่ปักหมุด
    FilterPinnedOnly,
    /// คำอธิบายปุ่มส่งเข้า canvas (P3-5)
    SendToCanvasHint,
    /// กดส่งเข้า canvas แล้วไม่มีอะไรขยับ
    NothingToApply,

    // ---- P4-2: บันทึกไฟล์ ----
    /// กำลังให้ผู้ใช้เลือกที่เก็บ
    SaveChoosing,
    /// กำลังเขียนไฟล์
    SaveInProgress,
    /// ผู้ใช้กดยกเลิกตอนเลือกที่เก็บ
    SaveCancelled,
    /// บันทึกไม่สำเร็จ
    SaveFailed,
    /// หัวข้อของแถบยืนยันตอนปิดทั้งที่ยังไม่ได้บันทึก
    CloseUnsavedTitle,
    /// ปุ่ม "บันทึกแล้วปิด"
    CloseSaveFirst,
    /// ปุ่ม "ไม่ปิดแล้ว"
    CloseCancel,
    /// ปุ่ม "ปิดโดยไม่บันทึก"
    CloseDiscard,
    /// คำเตือนของปุ่มปิดโดยไม่บันทึก
    CloseDiscardHint,

    // ---- P4-4: กู้คืนงานที่ยังไม่เคยบันทึก ----
    /// หัวข้อของแถบกู้คืน
    RecoverTitle,
    /// ปุ่ม "กู้คืน"
    RecoverRestore,
    /// ★ ปุ่ม "เก็บไว้ก่อน" — ตัวที่ docs/07 §4 บอกว่าสำคัญที่สุด
    RecoverLater,
    /// คำอธิบายของปุ่ม "เก็บไว้ก่อน" — ต้องบอกให้ชัดว่า **ไม่มีอะไรถูกลบ**
    RecoverLaterHint,
    /// ปุ่ม "ทิ้งไป"
    RecoverDiscard,
    /// คำเตือนของปุ่มทิ้ง
    RecoverDiscardHint,
    /// เวลาที่เขียนไฟล์ที่ระบบไฟล์ไม่ยอมบอก
    RecoverWhenUnknown,
    /// กู้คืนแล้ว แต่ยังไม่ได้บันทึกลงไฟล์จริง
    RecoveredNotSavedYet,
    /// ★★ หัวข้อของแถบกู้คืนตอนที่**เอกสารที่เพิ่งเปิด**มีของค้าง (P4-3)
    RecoverDocTitle,
    /// คำอธิบายของ "เก็บไว้ก่อน" กรณีเอกสาร — ต้องบอกว่าการแก้งานต่อจะเขียนทับ
    RecoverDocLaterHint,
    /// เอากลับมาแล้ว และมันยังไม่อยู่ในไฟล์ของมัน
    RecoveredIntoDocument,
    /// เปิดไฟล์ไม่สำเร็จ
    OpenFailed,
    /// กำลังเปิดไฟล์ที่ผู้ใช้เลือก
    OpenInProgress,
    /// กำลังรอผู้ใช้เลือกไฟล์ที่จะเปิด
    OpenChoosing,
    /// ★ คำอธิบายจุดบนแท็บตอนงานยังไม่ถูกบันทึก (docs/03 §1 — ตัวบ่งชี้ถาวร)
    UnsavedHint,
    /// คำอธิบายแท็บตอนทุกอย่างลงไฟล์แล้ว
    SavedHint,

    // ---- P4-5: packed mode ----
    /// ★ กำลังถามว่าจะบันทึกเป็นแบบไหน (สถานะบนแถบล่าง)
    SaveModeAsk,
    /// หัวข้อของแถบ "บันทึกเป็นแบบไหน"
    SaveModeTitle,
    /// ปุ่ม "ลิงก์ไปไฟล์เดิม"
    SaveModeLinked,
    /// คำอธิบายของ linked — ต้องบอกทั้งข้อดีและราคาที่จ่าย
    SaveModeLinkedHint,
    /// ปุ่ม "เก็บภาพไว้ในไฟล์"
    SaveModePacked,
    /// คำอธิบายของ packed
    SaveModePackedHint,
    /// ★★★ ตัวบ่งชี้ถาวร: ภาพอยู่นอกไฟล์ (`docs/07 §2`)
    StorageLinked,
    /// ตัวบ่งชี้ถาวร: ภาพทุกใบอยู่ในไฟล์
    StoragePacked,
    /// คำอธิบาย + วิธีเปลี่ยนของตัวบ่งชี้ตอนเป็น linked
    StorageLinkedHint,
    /// คำอธิบาย + วิธีเปลี่ยนของตัวบ่งชี้ตอนเป็น packed
    StoragePackedHint,
    /// คำอธิบายตอนเอกสารยังไม่มีไฟล์ — ตัวเลือกมีผลตอนบันทึกครั้งแรก
    StorageUnsavedHint,

    // ---- P3-7: group / ungroup ----
    /// หัวข้อกลุ่มในแผง Arrange
    GroupTitle,
    /// สิ่งที่เลือกไม่ได้อยู่ในกลุ่มไหน
    GroupNone,
    /// ★ คำตั้งต้นของชื่อกลุ่มอัตโนมัติ — ถูก **persist ลงไฟล์** จึงเป็นข้อมูล
    /// ของผู้ใช้ ไม่ใช่ป้ายบนหน้าจอ (ตั้งตอนสร้างด้วยภาษาที่ผู้ใช้ใช้อยู่ตอนนั้น)
    GroupDefaultName,
    /// คำอธิบายช่องเปลี่ยนชื่อกลุ่ม
    GroupRenameHint,
    /// ปุ่มยุบ/กางกลุ่ม
    GroupCollapsed,
    /// คำอธิบายปุ่มยุบ
    GroupCollapsedHint,
    /// สิ่งที่เลือกอยู่คนละกลุ่มกัน
    GroupMixed,
}

/// ข้อความภาษาอังกฤษ — **ต้องมีครบทุก key เสมอ** (เป็นตัวสำรองสุดท้าย)
fn en(key: Key) -> &'static str {
    match key {
        Key::Ready => "Ready",
        Key::RunningWithoutCache => "Running without a thumbnail cache",
        Key::UntitledBoard => "Untitled board",
        Key::NewBoardHint => "New board (P4-7)",
        Key::Library => "Library",
        Key::LibraryPlaceholder => "Image folders appear here",
        Key::LibraryDropHint => "Drag images in, or press Ctrl+V",
        Key::Inspector => "Inspector",
        Key::InspectorCanvasGeometry => "X / Y / W / H",
        Key::InspectorCanvasTransform => "Rotation, opacity, crop",
        Key::InspectorArrangeMeta => "Tags, rating, colour label",
        Key::InspectorArrangeGroup => "Group, notes",
        Key::ToolSelect => "Select",
        Key::ToolMove => "Move",
        Key::ToolCrop => "Crop",
        Key::ToolPicker => "Picker",
        Key::ToolMeasure => "Measure",
        Key::ToolText => "Note",
        Key::ToolGrayscale => "Grayscale",
        Key::ToolSort => "Sort",
        Key::ToolFilter => "Filter",
        Key::ToolTag => "Tag",
        Key::ToolSendToCanvas => "Send to Canvas",
        Key::ReadingClipboard => "Reading the clipboard…",
        Key::NothingToUndo => "Nothing left to undo",
        Key::NothingToRedo => "Nothing left to redo",
        Key::NothingToDelete => "Nothing to delete — select an unlocked image first",
        Key::InspectorNoSelection => "Select an image to adjust it",
        Key::FindFile => "Find the file…",
        Key::FindFileChoosing => "Choose the image file RefX could not find",
        Key::Opacity => "Opacity",
        Key::Invert => "Invert",
        Key::Brightness => "Brightness",
        Key::Contrast => "Contrast",
        Key::Flip => "Flip",
        Key::AlignLeft => "Align left",
        Key::AlignCentreX => "Align centre (horizontal)",
        Key::AlignRight => "Align right",
        Key::AlignTop => "Align top",
        Key::AlignCentreY => "Align middle (vertical)",
        Key::AlignBottom => "Align bottom",
        Key::DistributeX => "Distribute horizontally",
        Key::DistributeY => "Distribute vertically",
        Key::NothingToArrange => "Select at least two images to arrange them",
        Key::ReadingColour => "Reading the colour from the original file…",
        Key::NothingToPick => "Nothing there to pick a colour from",
        Key::ColourUnavailable => "Could not read the colour: the original file is unavailable",
        Key::Note => "Note",
        Key::NoteHint => "Type a note",
        Key::Rating => "Rating",
        Key::ColorLabelTitle => "Colour label",
        Key::ColorLabelNone => "No label",
        Key::ColorLabelUnknown => "This label came from a newer version - it is kept as it is",
        Key::Pinned => "Pinned (arrange will not move it)",
        Key::Tags => "Tags",
        Key::AddTagHint => "New tag",
        Key::RemoveTagHint => "Click to remove this tag",
        Key::MetaNote => "Note",
        Key::SortAddedAt => "Date added",
        Key::SortName => "File name",
        Key::SortRating => "Rating",
        Key::SortColorLabel => "Colour label",
        Key::SortAspect => "Aspect ratio",
        Key::SortModifiedAt => "Date modified",
        Key::SortFileSize => "File size",
        Key::SortCanvasOrder => "Canvas order",
        Key::SortAscending => "Low to high",
        Key::SortDescending => "High to low",
        Key::FilterAny => "Any",
        Key::FilterSearchHint => "Search name or note",
        Key::FilterClear => "Clear",
        Key::FilterMinRating => "Stars",
        Key::FilterPinnedOnly => "Pinned only",
        Key::SendToCanvasHint => {
            "Move these images on the canvas to match this arrangement (one undo puts them back)"
        }
        Key::SaveChoosing => "Choose where to save",
        Key::SaveInProgress => "Saving",
        Key::SaveCancelled => "Save cancelled",
        Key::SaveFailed => "Could not save - your work is still open, try another location",
        Key::CloseUnsavedTitle => "This board has unsaved changes",
        Key::CloseSaveFirst => "Save and close",
        Key::CloseCancel => "Keep working",
        Key::CloseDiscard => "Close without saving",
        Key::CloseDiscardHint => "Everything since the last save will be lost",
        Key::RecoverTitle => "Unsaved work from last time",
        Key::RecoverRestore => "Bring it back",
        Key::RecoverLater => "Keep it, decide later",
        Key::RecoverLaterHint => "Nothing is deleted - you will be asked again next time",
        Key::RecoverDiscard => "Throw it away",
        Key::RecoverDiscardHint => "That work is deleted for good",
        Key::RecoverWhenUnknown => "an earlier session",
        Key::RecoveredNotSavedYet => {
            "Restored - this board still has no file, press Ctrl+S to keep it"
        }
        Key::RecoverDocTitle => "This board has changes that never reached the file",
        Key::RecoverDocLaterHint => {
            "Nothing is deleted right now - but editing this board overwrites it in seconds"
        }
        Key::RecoveredIntoDocument => {
            "Brought the unsaved changes back - press Ctrl+S to write them to the file"
        }
        Key::OpenFailed => {
            "Could not open that board - the file may be damaged or from a newer RefX"
        }
        Key::OpenInProgress => "Opening",
        Key::OpenChoosing => "Choose a board to open",
        Key::UnsavedHint => "Not saved to a file yet - press Ctrl+S to keep this work",
        Key::SavedHint => "Everything is saved to the file",
        Key::SaveModeAsk => "Choose how the images should be stored",
        Key::SaveModeTitle => "Where should the images live?",
        Key::SaveModeLinked => "Link to the image files",
        Key::SaveModeLinkedHint => {
            "Small file. The images stay where they are, so this board needs them \
             to still be on this computer - pasted images are always stored inside anyway"
        }
        Key::SaveModePacked => "Store the images inside",
        Key::SaveModePackedHint => {
            "Big file, but it carries every image with it - use this to send the board \
             to someone else, back it up, or move it to another computer"
        }
        Key::StorageLinked => "Linked",
        Key::StoragePacked => "Packed",
        Key::StorageLinkedHint => {
            "The rest of the images are files on this computer. \
             Click to store every image inside the board file instead."
        }
        Key::StoragePackedHint => {
            "This board file carries its images with it - it opens anywhere. \
             Click to link them to their files instead."
        }
        Key::StorageUnsavedHint => "This is how the images will be stored when you save.",
        Key::GroupTitle => "Group",
        Key::GroupNone => "Not in a group",
        Key::GroupDefaultName => "Group",
        Key::GroupRenameHint => "Rename this group",
        Key::GroupCollapsed => "Collapsed",
        Key::GroupCollapsedHint => "Show this group as a single tile in Arrange",
        Key::GroupMixed => "Selection spans several groups",
        Key::NothingToApply => "Nothing moved — the images are already arranged like this",
    }
}

/// ข้อความภาษาไทย — **ไม่ครบก็ได้** ตัวที่ยังไม่มีคืน `None` แล้วตกกลับเป็นอังกฤษ
fn th(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Ready => "พร้อมใช้งาน",
        Key::RunningWithoutCache => "ใช้งานได้ แต่ไม่มี cache ภาพย่อ",
        Key::UntitledBoard => "board ที่ยังไม่ได้ตั้งชื่อ",
        Key::NewBoardHint => "เปิด board ใหม่ (P4-7)",
        Key::Library => "คลังภาพ",
        Key::LibraryPlaceholder => "โฟลเดอร์ภาพจะมาอยู่ตรงนี้",
        Key::LibraryDropHint => "ลากไฟล์ภาพเข้ามา หรือกด Ctrl+V",
        Key::Inspector => "รายละเอียด",
        Key::InspectorCanvasGeometry => "X / Y / กว้าง / สูง",
        Key::InspectorCanvasTransform => "หมุน, ความทึบ, ครอป",
        Key::InspectorArrangeMeta => "แท็ก, เรตติ้ง, ป้ายสี",
        Key::InspectorArrangeGroup => "กลุ่ม, โน้ต",
        Key::ToolSelect => "เลือก",
        Key::ToolMove => "ย้าย",
        Key::ToolCrop => "ครอป",
        Key::ToolPicker => "จิ้มสี",
        Key::ToolMeasure => "ไม้บรรทัด",
        Key::ToolText => "โน้ต",
        Key::ToolGrayscale => "ขาวดำ",
        Key::ToolSort => "เรียง",
        Key::ToolFilter => "กรอง",
        Key::ToolTag => "ติดแท็ก",
        Key::ToolSendToCanvas => "ส่งเข้า Canvas",
        Key::ReadingClipboard => "กำลังอ่าน clipboard…",
        Key::NothingToUndo => "ไม่มีอะไรให้ย้อนกลับแล้ว",
        Key::NothingToRedo => "ไม่มีอะไรให้ทำซ้ำแล้ว",
        Key::NothingToDelete => "ไม่มีอะไรให้ลบ — เลือกภาพที่ไม่ได้ล็อกไว้ก่อน",
        Key::InspectorNoSelection => "เลือกภาพก่อนถึงจะปรับได้",
        Key::FindFile => "หาไฟล์เอง…",
        Key::FindFileChoosing => "เลือกไฟล์ภาพที่ RefX หาไม่เจอ",
        Key::Opacity => "ความทึบ",
        Key::Invert => "กลับสี",
        Key::Brightness => "ความสว่าง",
        Key::Contrast => "คอนทราสต์",
        Key::Flip => "พลิกภาพ",
        Key::AlignLeft => "ชิดซ้าย",
        Key::AlignCentreX => "กึ่งกลางแนวนอน",
        Key::AlignRight => "ชิดขวา",
        Key::AlignTop => "ชิดบน",
        Key::AlignCentreY => "กึ่งกลางแนวตั้ง",
        Key::AlignBottom => "ชิดล่าง",
        Key::DistributeX => "กระจายแนวนอน",
        Key::DistributeY => "กระจายแนวตั้ง",
        Key::NothingToArrange => "เลือกอย่างน้อยสองภาพก่อนถึงจะจัดเรียงได้",
        Key::ReadingColour => "กำลังอ่านสีจากไฟล์ต้นฉบับ…",
        Key::NothingToPick => "ตรงนั้นไม่มีภาพให้อ่านสี",
        Key::ColourUnavailable => "อ่านสีไม่ได้: เปิดไฟล์ต้นฉบับไม่ได้",
        Key::Note => "โน้ต",
        Key::NoteHint => "พิมพ์โน้ตที่นี่",
        Key::Rating => "ดาว",
        Key::ColorLabelTitle => "ป้ายสี",
        Key::ColorLabelNone => "ไม่มีป้าย",
        Key::ColorLabelUnknown => "ป้ายนี้มาจากรุ่นที่ใหม่กว่า — เก็บไว้ตามเดิม",
        Key::Pinned => "ปักหมุด (arrange จะไม่ย้าย)",
        Key::Tags => "แท็ก",
        Key::AddTagHint => "แท็กใหม่",
        Key::RemoveTagHint => "กดเพื่อถอดแท็กนี้",
        Key::MetaNote => "โน้ต",
        Key::SortAddedAt => "เวลาที่เพิ่ม",
        Key::SortName => "ชื่อไฟล์",
        Key::SortRating => "ดาว",
        Key::SortColorLabel => "ป้ายสี",
        Key::SortAspect => "สัดส่วนภาพ",
        Key::SortModifiedAt => "วันที่แก้ไข",
        Key::SortFileSize => "ขนาดไฟล์",
        Key::SortCanvasOrder => "ตำแหน่งบน canvas",
        Key::SortAscending => "น้อยไปมาก",
        Key::SortDescending => "มากไปน้อย",
        Key::FilterAny => "ทั้งหมด",
        Key::FilterSearchHint => "ค้นชื่อไฟล์หรือโน้ต",
        Key::FilterClear => "ล้าง",
        Key::FilterMinRating => "ดาว",
        Key::FilterPinnedOnly => "เฉพาะที่ปักหมุด",
        Key::SendToCanvasHint => "ย้ายภาพบน canvas ให้เรียงแบบนี้ (กด Ctrl+Z ครั้งเดียวคืนสภาพเดิม)",
        Key::SaveChoosing => "เลือกที่เก็บไฟล์",
        Key::SaveInProgress => "กำลังบันทึก",
        Key::SaveCancelled => "ยกเลิกการบันทึกแล้ว",
        Key::SaveFailed => "บันทึกไม่สำเร็จ — งานของคุณยังเปิดอยู่ ลองเลือกที่เก็บอื่น",
        Key::CloseUnsavedTitle => "กระดานนี้มีการแก้ที่ยังไม่ได้บันทึก",
        Key::CloseSaveFirst => "บันทึกแล้วปิด",
        Key::CloseCancel => "ทำงานต่อ",
        Key::CloseDiscard => "ปิดโดยไม่บันทึก",
        Key::CloseDiscardHint => "ทุกอย่างตั้งแต่บันทึกครั้งล่าสุดจะหายไป",
        Key::RecoverTitle => "เจองานที่ยังไม่ได้บันทึกจากรอบก่อน",
        Key::RecoverRestore => "เอากลับมา",
        Key::RecoverLater => "เก็บไว้ก่อน ตัดสินใจทีหลัง",
        Key::RecoverLaterHint => "ไม่มีอะไรถูกลบ — จะถามใหม่ในรอบหน้า",
        Key::RecoverDiscard => "ทิ้งไป",
        Key::RecoverDiscardHint => "งานชุดนั้นจะถูกลบถาวร",
        Key::RecoverWhenUnknown => "รอบก่อน",
        Key::RecoveredNotSavedYet => "เอากลับมาแล้ว — กระดานนี้ยังไม่มีไฟล์ กด Ctrl+S เพื่อเก็บไว้",
        Key::RecoverDocTitle => "กระดานนี้มีการแก้ที่ยังไม่เคยถูกเขียนลงไฟล์",
        Key::RecoverDocLaterHint => {
            "ตอนนี้ยังไม่มีอะไรถูกลบ — แต่ถ้าแก้กระดานนี้ต่อ งานชุดนั้นจะถูกเขียนทับภายในไม่กี่วินาที ตัดสินใจก่อนทำงานต่อ"
        }
        Key::RecoveredIntoDocument => "เอาการแก้ที่ค้างอยู่กลับมาแล้ว — กด Ctrl+S เพื่อเขียนลงไฟล์",
        Key::OpenFailed => "เปิดกระดานไม่ได้ — ไฟล์อาจเสียหาย หรือถูกเขียนด้วย RefX รุ่นใหม่กว่า",
        Key::OpenInProgress => "กำลังเปิด",
        Key::OpenChoosing => "เลือกกระดานที่จะเปิด",
        Key::UnsavedHint => "ยังไม่ได้บันทึกลงไฟล์ — กด Ctrl+S เพื่อเก็บงานนี้ไว้",
        Key::SavedHint => "ทุกอย่างถูกบันทึกลงไฟล์แล้ว",
        Key::SaveModeAsk => "เลือกว่าจะเก็บภาพไว้แบบไหน",
        Key::SaveModeTitle => "จะเก็บภาพไว้ที่ไหน",
        Key::SaveModeLinked => "ลิงก์ไปไฟล์ภาพ",
        Key::SaveModeLinkedHint => {
            "ไฟล์เล็ก · ภาพยังอยู่ที่เดิมของมัน กระดานนี้จึงต้องใช้บนเครื่องที่มีภาพอยู่ \
             — ภาพที่วางจาก clipboard ถูกเก็บไว้ข้างในให้เสมออยู่แล้ว"
        }
        Key::SaveModePacked => "เก็บภาพไว้ในไฟล์",
        Key::SaveModePackedHint => {
            "ไฟล์ใหญ่ แต่พกภาพไปด้วยทุกใบ — ใช้ตอนส่งกระดานให้คนอื่น สำรองไว้ หรือย้ายเครื่อง"
        }
        Key::StorageLinked => "ลิงก์ภาพ",
        Key::StoragePacked => "ภาพอยู่ในไฟล์",
        Key::StorageLinkedHint => "ภาพที่เหลือเป็นไฟล์บนเครื่องนี้ · กดเพื่อเก็บภาพทุกใบไว้ในไฟล์กระดานแทน",
        Key::StoragePackedHint => "ไฟล์กระดานนี้พกภาพไปด้วย เปิดที่ไหนก็ได้ · กดเพื่อกลับไปลิงก์ไฟล์ภาพแทน",
        Key::StorageUnsavedHint => "ภาพจะถูกเก็บแบบนี้ตอนบันทึก",
        Key::GroupTitle => "กลุ่ม",
        Key::GroupNone => "ไม่ได้อยู่ในกลุ่มไหน",
        Key::GroupDefaultName => "กลุ่ม",
        Key::GroupRenameHint => "เปลี่ยนชื่อกลุ่มนี้",
        Key::GroupCollapsed => "ยุบอยู่",
        Key::GroupCollapsedHint => "ยุบกลุ่มนี้ให้เหลือใบเดียวในโหมด Arrange",
        Key::GroupMixed => "สิ่งที่เลือกอยู่คนละกลุ่มกัน",
        Key::NothingToApply => "ไม่มีอะไรขยับ — ภาพเรียงแบบนี้อยู่แล้ว",
    })
}

/// ★ กติกาตกกลับ — จุดเดียวที่ตัดสินว่าจะแสดงอะไรเมื่อคำแปลยังไม่มี
///
/// แยกเป็นฟังก์ชันเพื่อให้ **ทดสอบเส้นทางตกกลับได้จริง** ไม่ใช่แค่เชื่อว่ามันทำงาน
#[must_use]
fn pick(translated: Option<&'static str>, fallback: &'static str) -> &'static str {
    translated.unwrap_or(fallback)
}

/// ข้อความคงที่ตามภาษาที่เลือก
///
/// คำแปลที่ยังไม่มีจะได้อังกฤษกลับไป — **ไม่มีทางได้ key หรือช่องว่าง**
#[must_use]
pub fn t(lang: Lang, key: Key) -> &'static str {
    match lang {
        Lang::En => en(key),
        Lang::Th => pick(th(key), en(key)),
    }
}

/// เทมเพลตที่มีตัวแปร — ค่าที่ใส่ได้มีอะไรบ้างเขียนไว้ในคอมเมนต์ของแต่ละตัว
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// `{n}` — จำนวน item บน board
    ItemCount,
    /// `{n}` — จำนวนสมาชิกของกลุ่มที่เลือกอยู่ (P3-7)
    GroupMembers,
    /// `{name}` — บันทึกลงไฟล์นี้สำเร็จแล้ว (P4-2)
    Saved,
    /// `{items}` `{when}` — เจองานค้างจาก session ก่อนกี่ชิ้น เขียนไว้เมื่อไหร่ (P4-4)
    RecoverFound,
    /// `{name}` — เปิดไฟล์นี้สำเร็จแล้ว (P4-4)
    Opened,
    /// `{pct}` — ระดับซูมเป็นเปอร์เซ็นต์
    Zoom,
    /// `{n}` — จำนวนเฟรมที่วาดไปแล้ว (ตัวชี้วัด I-1 ที่เห็นด้วยตา)
    FramesDrawn,
    /// `{used}` `{limit}` — RAM ของ decode pool
    Ram,
    /// `{used}` `{limit}` — VRAM ของ texture
    Vram,
    /// `{n}` `{size}` — จำนวนภาพใน cache และขนาดไฟล์ DB
    CacheSummary,
    /// `{n}` — งาน decode ที่ยังค้างคิว
    DecodeQueued,
    /// `{n}` — งาน decode ที่ถูกยกเลิกไปแล้ว
    DecodeCancelled,
    /// `{used}` `{limit}` `{calls}` `{evicted}` — working texture ชั้น B (docs/04 §4)
    WorkingTextures,
    /// จำนวน texture upload สะสม (หลักฐานเกณฑ์ P2-8)
    AtlasUploads,
    /// `{drawn}` `{view}` `{total}` `{examined}` — virtual scrolling (หลักฐานเกณฑ์ P3-3)
    ArrangeDrawn,
    /// `{done}` `{total}` — ความคืบหน้าการโหลด (docs/05 §6 เงื่อนไขข้อ 3)
    Loading,
    /// `{n}` — กำลังเปิดไฟล์กี่ไฟล์
    OpeningFiles,
    /// `{n}` `{ms}` — สรุปเวลาหลังเปิดไฟล์ครบ
    OpenedFiles,
    /// `{n}` — จัดลง canvas แล้วกี่ใบ (P3-5)
    LayoutApplied,
    /// `{shown}` `{total}` — ตัวกรองซ่อนบางใบอยู่ (P3-4)
    ///
    /// ★ ต้องเห็นได้เสมอตอนกรองอยู่ — ผู้ใช้ที่มองหาภาพที่ "หายไป" ต้องรู้ทันที
    /// ว่ามันถูกกรอง ไม่ใช่หาย (ไม่งั้นเขาจะสรุปว่าโปรแกรมทำงานหาย)
    FilterShowing,
    /// `{capacity}` `{rejected}` `{requested}` — board เต็ม เพิ่มไม่ครบ (ROADMAP P3-3)
    ///
    /// ★ ต้องบอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ** (CLAUDE.md) — ผู้ใช้ที่ลาก
    /// 10,000 ไฟล์แล้วได้ 3,072 ใบต้องไม่ต้องเดาเองว่าที่เหลือหายไปไหน
    BoardFull,
    /// `{mode}` — สลับโหมดแล้ว
    SwitchedMode,
    /// `{what}` `{when}` — ฟีเจอร์ที่ยังไม่ได้ทำ
    NotImplemented,
    /// `{mb}` `{limit}` — ไฟล์ใหญ่เกินเพดาน
    ErrFileTooLarge,
    /// `{w}` `{h}` `{pixels}` `{limit}` — ภาพมี pixel เกินเพดาน
    ErrImageTooLarge,
    /// ไม่รู้จักชนิดไฟล์
    ErrUnknownFormat,
    /// `{format}` — รู้จัก format แต่ยังเปิดไม่ได้
    ErrFormatNotAllowed,
    /// header อ่านไม่ได้
    ErrBadHeader,
    /// decoder พัง แต่ถูกดักไว้แล้ว (I-7)
    ErrDecoderPanic,
    /// decode ล้มด้วยสาเหตุปกติ
    ErrDecode,
    /// `{file}` — อ่านไฟล์จากดิสก์ไม่ได้
    ErrReadFile,
    /// `{file}` — ที่อยู่นี้ไม่ใช่ไฟล์
    ErrNotAFile,
    /// `{file}` `{seconds}` — ใช้เวลานานเกินเพดาน
    ErrTimeout,
    /// `{layers}` — atlas เต็ม
    ErrAtlasFull,
    /// `{requested}` `{used}` `{limit}` — VRAM ไม่พอ
    ErrOutOfVram,
    /// `{ms}` — วางภาพจาก clipboard สำเร็จ
    PastedImage,
    /// ใน clipboard ไม่มีภาพและไม่มีไฟล์
    ErrClipboardEmpty,
    /// โปรแกรมอื่นถือ clipboard อยู่
    ErrClipboardBusy,
    /// เครื่องนี้ใช้ clipboard ไม่ได้
    ErrClipboardUnavailable,
    /// มีภาพอยู่แต่อ่านไม่ออก
    ErrClipboardUndecodable,
    /// `{w}` `{h}` — ภาพดิบมีจำนวนไบต์ไม่ตรงกับขนาดที่ประกาศ
    ErrMalformedPixels,
    /// `{mb}` `{cap}` — ที่พักของภาพที่วางเกินเพดาน แต่ทุกไฟล์ยังมีคนอ้างถึงอยู่
    SpoolOverCap,
    /// `{file}` `{n}` — ภาพที่หาไฟล์ไม่เจอ (แสดงใน inspector)
    MissingImage,
    /// `{found}` `{total}` — สรุปผลการตามหาไฟล์ตอนเปิดเอกสาร
    RelinkFound,
    /// `{n}` — เปิดเอกสารแล้วมีภาพที่หาไฟล์ไม่เจอ
    RelinkMissing,
    /// ★ `{n}` — ภาพที่แกะออกมาจากตัวเอกสารเอง (packed — P4-5)
    RelinkUnpacked,
    /// `{inside}` `{images}` — กี่ใบที่อยู่ในไฟล์งานแล้ว (ตัวบ่งชี้โหมด)
    StorageInside,
}

/// เทมเพลตภาษาอังกฤษ — ต้องมีครบทุกตัว
fn template_en(template: Template) -> &'static str {
    match template {
        Template::ItemCount => "{n} items",
        Template::GroupMembers => "{n} in this group",
        Template::Saved => "Saved to {name}",
        Template::RecoverFound => "{items} items from {when}",
        Template::Opened => "Opened {name}",
        Template::Zoom => "Zoom {pct}%",
        Template::FramesDrawn => "Frames {n}",
        Template::Ram => "RAM {used} / {limit}",
        Template::Vram => "VRAM {used} / {limit}",
        Template::CacheSummary => "Cache {n} images ({size})",
        Template::DecodeQueued => "Decode queue {n}",
        Template::DecodeCancelled => "Cancelled {n}",
        Template::WorkingTextures => "Sharp {used} / {limit} · {calls} draws · {evicted} evicted",
        Template::AtlasUploads => "{uploads} uploads",
        Template::ArrangeDrawn => "drawn {drawn} (in view {view}) of {total} · checked {examined}",
        Template::Loading => "Loading {done} / {total}",
        Template::OpeningFiles => "Opening {n} files…",
        Template::OpenedFiles => "Opened {n} files in {ms} ms",
        Template::LayoutApplied => "Arranged {n} images on the canvas",
        Template::FilterShowing => "showing {shown} of {total}",
        Template::BoardFull => {
            "This board is full at {capacity} images — {rejected} of the {requested} you opened could not be added. Try splitting them across several boards."
        }
        Template::SwitchedMode => "Switched to {mode} mode",
        Template::NotImplemented => "{what} is not available yet — planned for {when}",
        Template::ErrFileTooLarge => {
            "This file is too large ({mb} MB, the limit is {limit} MB)\n\
             Try shrinking the image before adding it."
        }
        Template::ErrImageTooLarge => {
            "This image is too large ({w}×{h} = {pixels} pixels, the limit is {limit})\n\
             If you expected a smaller image, the file may be damaged or altered."
        }
        Template::ErrUnknownFormat => {
            "RefX does not recognise this file type\n\
             It can open PNG, JPEG, WebP, GIF, BMP, TGA and TIFF."
        }
        Template::ErrFormatNotAllowed => {
            "RefX cannot open {format} files yet\nTry converting the image to PNG or JPEG."
        }
        Template::ErrBadHeader => {
            "The file header could not be read — the file is damaged or incomplete\n\
             Try opening it in the program that made it and saving a fresh copy."
        }
        Template::ErrDecoderPanic => {
            "This file broke the image decoder — it was skipped.\nEvery other image still works."
        }
        Template::ErrDecode => {
            "This image could not be opened — the file may be damaged or incomplete\n\
             Try opening it in the program that made it and saving a fresh copy."
        }
        Template::ErrReadFile => {
            "Could not read {file}\n\
             If it lives on OneDrive or Dropbox, wait for that folder to finish syncing."
        }
        Template::ErrNotAFile => {
            "{file} is not an image file
Drag in a PNG, JPEG, WebP, GIF, BMP, TGA or TIFF instead."
        }
        Template::ErrTimeout => {
            "Opening {file} took longer than {seconds} seconds — it was skipped for now\n\
             If the file is on a network or cloud drive, copy it to this computer first."
        }
        Template::ErrAtlasFull => {
            "The thumbnail storage is full (all {layers} layers are in use)\n\
             Close a board you are not using, or raise the memory limit in Settings."
        }
        Template::ErrOutOfVram => {
            "The graphics card is out of memory ({requested} MB requested, {used} MB of {limit} MB in use)\n\
             Close a board you are not using, or raise the memory limit in Settings."
        }
        Template::PastedImage => "Pasted an image from the clipboard in {ms} ms",
        Template::SpoolOverCap => {
            "Pasted images are using {mb} MB of space (the limit is {cap} MB), and none of them can be cleared yet\n\
             They belong to work that has not been saved — save your boards, then restart RefX to free the space."
        }
        Template::MissingImage => {
            "{file} could not be found ({n} selected)\n\
             The image is still on the board — point RefX at the file and the rest of that folder follows."
        }
        Template::RelinkFound => "Found {found} of {total} images that had moved",
        Template::RelinkMissing => {
            "{n} images could not be found\n\
             Select one, then use Find the file — the rest of that folder is matched for you."
        }
        Template::RelinkUnpacked => "Loaded {n} images stored inside this board file",
        Template::StorageInside => "{inside} of {images} images are inside the board file",
        Template::ErrClipboardEmpty => {
            "There is no image in the clipboard\n\
             Copy an image or an image file first, or drag the file into the window."
        }
        Template::ErrClipboardBusy => {
            "Another program is holding the clipboard right now\n\
             Wait a moment and press Ctrl+V again."
        }
        Template::ErrClipboardUnavailable => {
            "RefX cannot reach the clipboard on this computer\n\
             Drag the image into the window instead — that works exactly the same way."
        }
        Template::ErrClipboardUndecodable => {
            "There is something in the clipboard, but it is not an image RefX can read\n\
             Copy the image again, or save it as a PNG and drag that file in."
        }
        Template::ErrMalformedPixels => {
            "The pasted image is damaged ({w}×{h} with the wrong amount of data)\n\
             Copy it again from the program it came from."
        }
    }
}

/// เทมเพลตภาษาไทย — ไม่ครบก็ได้ ตัวที่ยังไม่มีตกกลับเป็นอังกฤษ
fn template_th(template: Template) -> Option<&'static str> {
    Some(match template {
        Template::ItemCount => "{n} รายการ",
        Template::GroupMembers => "{n} ใบในกลุ่มนี้",
        Template::Saved => "บันทึกลง {name} แล้ว",
        Template::RecoverFound => "{items} ชิ้น จาก{when}",
        Template::Opened => "เปิด {name} แล้ว",
        Template::Zoom => "ซูม {pct}%",
        Template::FramesDrawn => "เฟรมที่วาด {n}",
        Template::Ram => "RAM {used} / {limit}",
        Template::Vram => "VRAM {used} / {limit}",
        Template::CacheSummary => "cache {n} ภาพ ({size})",
        Template::DecodeQueued => "คิวถอดรหัส {n}",
        Template::DecodeCancelled => "ยกเลิกไป {n}",
        Template::WorkingTextures => "ภาพคม {used} / {limit} · วาด {calls} ครั้ง · ไล่ออก {evicted}",
        Template::AtlasUploads => "อัป texture {uploads} ครั้ง",
        Template::ArrangeDrawn => "วาด {drawn} (ในจอ {view}) จาก {total} · ตรวจ {examined}",
        Template::Loading => "กำลังโหลด {done} / {total}",
        Template::OpeningFiles => "กำลังเปิด {n} ไฟล์…",
        Template::OpenedFiles => "เปิด {n} ไฟล์ใน {ms} ms",
        Template::LayoutApplied => "จัด {n} ภาพลง canvas แล้ว",
        Template::FilterShowing => "กรองอยู่ {shown} จาก {total}",
        Template::BoardFull => {
            "board นี้เต็มที่ {capacity} ภาพ — เพิ่มอีก {rejected} ใบจาก {requested} ใบที่เปิดเข้ามาไม่ได้ ลองแยกเป็นหลาย board"
        }
        Template::SwitchedMode => "สลับไปโหมด {mode}",
        Template::NotImplemented => "{what} ยังทำไม่ได้ — รอ {when}",
        Template::ErrFileTooLarge => {
            "ไฟล์ใหญ่เกินไป ({mb} MB, รับได้ไม่เกิน {limit} MB)\n\
             ลองย่อภาพก่อนแล้วค่อยลากเข้ามาใหม่"
        }
        Template::ErrImageTooLarge => {
            "ภาพใหญ่เกินไป ({w}×{h} = {pixels} จุด, รับได้ไม่เกิน {limit} จุด)\n\
             ถ้าไฟล์นี้ควรจะเล็กกว่านี้ แปลว่าไฟล์อาจเสียหายหรือถูกดัดแปลงมา"
        }
        Template::ErrUnknownFormat => {
            "ไม่รู้จักชนิดของไฟล์นี้\nRefX เปิดได้เฉพาะ PNG, JPEG, WebP, GIF, BMP, TGA และ TIFF"
        }
        Template::ErrFormatNotAllowed => {
            "RefX ยังเปิดไฟล์ชนิด {format} ไม่ได้\nลองแปลงเป็น PNG หรือ JPEG ก่อน"
        }
        Template::ErrBadHeader => {
            "อ่านข้อมูลหัวไฟล์ไม่ได้ — ไฟล์น่าจะเสียหายหรือถูกตัดไม่ครบ\n\
             ลองเปิดด้วยโปรแกรมที่สร้างไฟล์นี้แล้วบันทึกใหม่อีกครั้ง"
        }
        Template::ErrDecoderPanic => {
            "ไฟล์นี้ทำให้ตัวถอดรหัสภาพทำงานผิดพลาด — ข้ามไฟล์นี้ไป\nไฟล์อื่นยังใช้ได้ตามปกติ"
        }
        Template::ErrDecode => {
            "เปิดภาพไม่ได้ — ไฟล์อาจเสียหายหรือถูกตัดไม่ครบ\n\
             ลองเปิดด้วยโปรแกรมที่สร้างไฟล์นี้แล้วบันทึกใหม่อีกครั้ง"
        }
        Template::ErrReadFile => {
            "อ่านไฟล์ {file} ไม่ได้\n\
             ถ้าไฟล์อยู่บน OneDrive หรือ Dropbox ลองเปิดโฟลเดอร์นั้นให้ sync เสร็จก่อน"
        }
        Template::ErrNotAFile => {
            "{file} ไม่ใช่ไฟล์ภาพ
ลากไฟล์ PNG, JPEG, WebP, GIF, BMP, TGA หรือ TIFF เข้ามาแทน"
        }
        Template::ErrTimeout => {
            "ใช้เวลาเปิดภาพ {file} นานเกิน {seconds} วินาที — ข้ามไฟล์นี้ไปก่อน\n\
             ถ้าไฟล์อยู่บนไดรฟ์เครือข่ายหรือ cloud ลองคัดลอกมาไว้ในเครื่องก่อน"
        }
        Template::ErrAtlasFull => {
            "พื้นที่เก็บภาพย่อเต็ม (ใช้ครบ {layers} ชั้นแล้ว)\n\
             ลองปิด board ที่ไม่ได้ใช้ หรือเพิ่มเพดานหน่วยความจำในการตั้งค่า"
        }
        Template::ErrOutOfVram => {
            "หน่วยความจำการ์ดจอไม่พอ (ขอ {requested} MB, ใช้อยู่ {used} MB จากเพดาน {limit} MB)\n\
             ลองปิด board ที่ไม่ได้ใช้ หรือเพิ่มเพดานหน่วยความจำในการตั้งค่า"
        }
        Template::PastedImage => "วางภาพจาก clipboard ใน {ms} ms",
        Template::SpoolOverCap => {
            "ภาพที่วางไว้ใช้พื้นที่ {mb} MB (เพดาน {cap} MB) และยังลบอะไรไม่ได้เลยสักไฟล์\n\
             ทั้งหมดเป็นของงานที่ยังไม่ได้บันทึก — บันทึก board ให้เรียบร้อยแล้วเปิด RefX ใหม่ พื้นที่จะถูกคืน"
        }
        Template::MissingImage => {
            "หาไฟล์ {file} ไม่เจอ (เลือกไว้ {n} ใบ)\n\
             ภาพยังอยู่บน board — ชี้ไฟล์ให้ RefX แล้วที่เหลือในโฟลเดอร์นั้นจะตามมาเอง"
        }
        Template::RelinkFound => "หาไฟล์ที่ย้ายที่เจอ {found} จาก {total} ใบ",
        Template::RelinkMissing => {
            "หาไฟล์ไม่เจอ {n} ใบ\n\
             เลือกใบใดใบหนึ่งแล้วกดปุ่มหาไฟล์เอง — ที่เหลือในโฟลเดอร์นั้นจะถูกจับคู่ให้"
        }
        Template::RelinkUnpacked => "ใช้ภาพ {n} ใบที่เก็บอยู่ในไฟล์กระดานนี้",
        Template::StorageInside => "ภาพอยู่ในไฟล์กระดานแล้ว {inside} จาก {images} ใบ",
        Template::ErrClipboardEmpty => {
            "ใน clipboard ไม่มีภาพ\n\
             ลองก๊อปภาพหรือไฟล์ภาพมาก่อน หรือลากไฟล์เข้ามาในหน้าต่างก็ได้เหมือนกัน"
        }
        Template::ErrClipboardBusy => {
            "โปรแกรมอื่นถือ clipboard อยู่ตอนนี้\n\
             รอสักครู่แล้วกด Ctrl+V ใหม่อีกครั้ง"
        }
        Template::ErrClipboardUnavailable => {
            "RefX เข้าถึง clipboard ของเครื่องนี้ไม่ได้\n\
             ลากไฟล์ภาพเข้ามาในหน้าต่างแทนได้ ผลเหมือนกันทุกอย่าง"
        }
        Template::ErrClipboardUndecodable => {
            "ใน clipboard มีของอยู่ แต่ไม่ใช่ภาพที่ RefX อ่านได้\n\
             ลองก๊อปภาพใหม่อีกครั้ง หรือบันทึกเป็นไฟล์ PNG แล้วลากเข้ามา"
        }
        Template::ErrMalformedPixels => {
            "ภาพที่วางเข้ามาเสียหาย ({w}×{h} แต่ข้อมูลไม่ครบตามขนาด)\n\
             ลองก๊อปใหม่จากโปรแกรมต้นทางอีกครั้ง"
        }
    })
}

/// เทมเพลตตามภาษาที่เลือก (ตกกลับเป็นอังกฤษเหมือน [`t`])
#[must_use]
fn template(lang: Lang, template: Template) -> &'static str {
    match lang {
        Lang::En => template_en(template),
        Lang::Th => pick(template_th(template), template_en(template)),
    }
}

/// เติมค่าลงตัวยึดตำแหน่งของเทมเพลต
///
/// ใช้แทน `format!` เพราะ `format!` ต้องการ literal ตอนคอมไพล์ ซึ่งแปลตอนรันไม่ได้
/// วิธีนี้ทำให้ **ผู้แปลแตะแค่สตริง** และสลับลำดับคำได้ตามไวยากรณ์ของแต่ละภาษา
///
/// ตัวยึดที่ไม่มีใครเติมจะถูกทิ้งไว้ตามเดิม — เห็นได้ทันทีตอนทดสอบ ดีกว่าหายเงียบ
#[must_use]
pub fn fill(lang: Lang, which: Template, args: &[(&str, &str)]) -> String {
    let mut out = template(lang, which).to_owned();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// ข้อความสำหรับผู้ใช้เมื่อเปิดภาพไม่สำเร็จ
///
/// ประกอบจาก **ฟิลด์** ของ error ไม่ใช่จาก `Display` ของมัน — `Display`
/// เป็นภาษาอังกฤษสำหรับ log เสมอ (docs/03 §0)
#[must_use]
pub fn load_error(lang: Lang, err: &LoadError) -> String {
    match err {
        LoadError::FileTooLarge {
            actual_mb,
            limit_mb,
        } => fill(
            lang,
            Template::ErrFileTooLarge,
            &[
                ("mb", &actual_mb.to_string()),
                ("limit", &limit_mb.to_string()),
            ],
        ),
        LoadError::ImageTooLarge {
            width,
            height,
            pixels,
            limit,
        } => fill(
            lang,
            Template::ErrImageTooLarge,
            &[
                ("w", &width.to_string()),
                ("h", &height.to_string()),
                ("pixels", &pixels.to_string()),
                ("limit", &limit.to_string()),
            ],
        ),
        LoadError::UnknownFormat => fill(lang, Template::ErrUnknownFormat, &[]),
        LoadError::FormatNotAllowed { format } => fill(
            lang,
            Template::ErrFormatNotAllowed,
            &[("format", &format!("{format:?}"))],
        ),
        LoadError::BadHeader => fill(lang, Template::ErrBadHeader, &[]),
        LoadError::DecoderPanic => fill(lang, Template::ErrDecoderPanic, &[]),
        LoadError::Decode(_) => fill(lang, Template::ErrDecode, &[]),
        LoadError::Io { file, .. } => fill(lang, Template::ErrReadFile, &[("file", file)]),
        LoadError::NotAFile { file } => fill(lang, Template::ErrNotAFile, &[("file", file)]),
        LoadError::MalformedPixels { width, height, .. } => fill(
            lang,
            Template::ErrMalformedPixels,
            &[("w", &width.to_string()), ("h", &height.to_string())],
        ),
    }
}

/// ข้อความสำหรับผู้ใช้เมื่องาน decode ล้มเหลว
#[must_use]
pub fn job_failure(lang: Lang, err: &JobFailure) -> String {
    match err {
        JobFailure::Load(inner) => load_error(lang, inner),
        JobFailure::Timeout { file, seconds } => fill(
            lang,
            Template::ErrTimeout,
            &[("file", file), ("seconds", &seconds.to_string())],
        ),
        JobFailure::Clipboard(inner) => clipboard_error(lang, inner),
    }
}

/// ข้อความสำหรับผู้ใช้เมื่อวางจาก clipboard ไม่สำเร็จ
///
/// ★ ทุกกรณีต้องบอก **ทางออกที่ทำได้จริง** — "วางไม่ได้" เฉย ๆ ทำให้ผู้ใช้
/// นึกว่าโปรแกรมพัง ทั้งที่ส่วนใหญ่แค่ก๊อปข้อความมาแทนภาพ
#[must_use]
pub fn clipboard_error(lang: Lang, err: &ClipboardError) -> String {
    let template = match err {
        ClipboardError::NoImage => Template::ErrClipboardEmpty,
        ClipboardError::Busy => Template::ErrClipboardBusy,
        ClipboardError::Unavailable { .. } => Template::ErrClipboardUnavailable,
        ClipboardError::Undecodable => Template::ErrClipboardUndecodable,
    };
    fill(lang, template, &[])
}

/// ข้อความสำหรับผู้ใช้เมื่อเก็บภาพย่อลง atlas ไม่ได้
#[must_use]
pub fn atlas_error(lang: Lang, err: &AtlasError) -> String {
    match err {
        // ★ `NeedsResize` เป็นสัญญาณควบคุมภายใน — ชั้น UI จัดการเองหมดแล้ว
        //   (`RefxApp::upload_thumb` ขยาย atlas แล้วเติมของเดิมกลับให้)
        //   ถ้ามาถึงตรงนี้ได้แปลว่ามีเส้นทางที่ลืมจัดการ จึงบอกผู้ใช้แบบเดียวกับ
        //   "เต็ม" เพราะสิ่งที่เขาเห็นเหมือนกัน คือภาพนี้ขึ้นเป็นสี่เหลี่ยมสีเด่นแทน
        AtlasError::Full { layers } | AtlasError::NeedsResize { layers } => fill(
            lang,
            Template::ErrAtlasFull,
            &[("layers", &layers.to_string())],
        ),
        AtlasError::OutOfVram(refx_render::texture::VramError::OverBudget {
            requested_mb,
            used_mb,
            limit_mb,
        }) => fill(
            lang,
            Template::ErrOutOfVram,
            &[
                ("requested", &requested_mb.to_string()),
                ("used", &used_mb.to_string()),
                ("limit", &limit_mb.to_string()),
            ],
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// รายการ key ทั้งหมด — ต้องเติมเมื่อเพิ่ม key ใหม่
    const ALL_KEYS: &[Key] = &[
        Key::Ready,
        Key::RunningWithoutCache,
        Key::UntitledBoard,
        Key::NewBoardHint,
        Key::Library,
        Key::LibraryPlaceholder,
        Key::LibraryDropHint,
        Key::Inspector,
        Key::InspectorCanvasGeometry,
        Key::InspectorCanvasTransform,
        Key::InspectorArrangeMeta,
        Key::InspectorArrangeGroup,
        Key::ToolSelect,
        Key::ToolMove,
        Key::ToolCrop,
        Key::ToolPicker,
        Key::ToolMeasure,
        Key::ToolText,
        Key::ToolGrayscale,
        Key::ToolSort,
        Key::ToolFilter,
        Key::ToolTag,
        Key::ToolSendToCanvas,
        Key::ReadingClipboard,
        Key::NothingToUndo,
        Key::NothingToRedo,
        Key::NothingToDelete,
        Key::InspectorNoSelection,
        Key::FindFile,
        Key::FindFileChoosing,
        Key::Opacity,
        Key::Invert,
        Key::Brightness,
        Key::Contrast,
        Key::Flip,
        Key::AlignLeft,
        Key::AlignCentreX,
        Key::AlignRight,
        Key::AlignTop,
        Key::AlignCentreY,
        Key::AlignBottom,
        Key::DistributeX,
        Key::DistributeY,
        Key::NothingToArrange,
        Key::ReadingColour,
        Key::NothingToPick,
        Key::ColourUnavailable,
        Key::Note,
        Key::NoteHint,
        Key::Rating,
        Key::ColorLabelTitle,
        Key::ColorLabelNone,
        Key::ColorLabelUnknown,
        Key::Pinned,
        Key::Tags,
        Key::AddTagHint,
        Key::RemoveTagHint,
        Key::MetaNote,
        Key::SortAddedAt,
        Key::SortName,
        Key::SortRating,
        Key::SortColorLabel,
        Key::SortAspect,
        Key::SortModifiedAt,
        Key::SortFileSize,
        Key::SortCanvasOrder,
        Key::SortAscending,
        Key::SortDescending,
        Key::FilterAny,
        Key::FilterSearchHint,
        Key::FilterClear,
        Key::FilterMinRating,
        Key::FilterPinnedOnly,
        Key::SendToCanvasHint,
        Key::NothingToApply,
        Key::SaveChoosing,
        Key::SaveInProgress,
        Key::SaveCancelled,
        Key::SaveFailed,
        Key::CloseUnsavedTitle,
        Key::CloseSaveFirst,
        Key::CloseCancel,
        Key::CloseDiscard,
        Key::CloseDiscardHint,
        Key::RecoverTitle,
        Key::RecoverRestore,
        Key::RecoverLater,
        Key::RecoverLaterHint,
        Key::RecoverDiscard,
        Key::RecoverDiscardHint,
        Key::RecoverWhenUnknown,
        Key::RecoveredNotSavedYet,
        Key::RecoverDocTitle,
        Key::RecoverDocLaterHint,
        Key::RecoveredIntoDocument,
        Key::OpenFailed,
        Key::OpenInProgress,
        Key::OpenChoosing,
        Key::UnsavedHint,
        Key::SavedHint,
        Key::SaveModeAsk,
        Key::SaveModeTitle,
        Key::SaveModeLinked,
        Key::SaveModeLinkedHint,
        Key::SaveModePacked,
        Key::SaveModePackedHint,
        Key::StorageLinked,
        Key::StoragePacked,
        Key::StorageLinkedHint,
        Key::StoragePackedHint,
        Key::StorageUnsavedHint,
        Key::GroupTitle,
        Key::GroupNone,
        Key::GroupDefaultName,
        Key::GroupRenameHint,
        Key::GroupCollapsed,
        Key::GroupCollapsedHint,
        Key::GroupMixed,
    ];

    const ALL_TEMPLATES: &[Template] = &[
        Template::ItemCount,
        Template::GroupMembers,
        Template::Saved,
        Template::RecoverFound,
        Template::Opened,
        Template::Zoom,
        Template::FramesDrawn,
        Template::Ram,
        Template::Vram,
        Template::CacheSummary,
        Template::DecodeQueued,
        Template::DecodeCancelled,
        Template::WorkingTextures,
        Template::AtlasUploads,
        Template::ArrangeDrawn,
        Template::Loading,
        Template::OpeningFiles,
        Template::OpenedFiles,
        Template::LayoutApplied,
        Template::FilterShowing,
        Template::BoardFull,
        Template::SwitchedMode,
        Template::NotImplemented,
        Template::ErrFileTooLarge,
        Template::ErrImageTooLarge,
        Template::ErrUnknownFormat,
        Template::ErrFormatNotAllowed,
        Template::ErrBadHeader,
        Template::ErrDecoderPanic,
        Template::ErrDecode,
        Template::ErrReadFile,
        Template::ErrNotAFile,
        Template::ErrTimeout,
        Template::ErrAtlasFull,
        Template::ErrOutOfVram,
        Template::PastedImage,
        Template::ErrClipboardEmpty,
        Template::ErrClipboardBusy,
        Template::ErrClipboardUnavailable,
        Template::ErrClipboardUndecodable,
        Template::ErrMalformedPixels,
        Template::SpoolOverCap,
        Template::MissingImage,
        Template::RelinkFound,
        Template::RelinkMissing,
        Template::RelinkUnpacked,
        Template::StorageInside,
    ];

    /// ★ กฎข้อ 2 ของ docs/03 §0: ห้ามมีทางที่ผู้ใช้จะเห็นช่องว่างหรือชื่อ key
    #[test]
    fn no_string_is_ever_empty_in_either_language() {
        for &lang in &[Lang::En, Lang::Th] {
            for &key in ALL_KEYS {
                assert!(!t(lang, key).trim().is_empty(), "{lang:?} {key:?} ว่างเปล่า");
            }
            for &tpl in ALL_TEMPLATES {
                assert!(
                    !template(lang, tpl).trim().is_empty(),
                    "{lang:?} {tpl:?} ว่างเปล่า"
                );
            }
        }
    }

    /// ★★ ห้ามมีช่องว่างติดกันหรือขึ้นบรรทัดกลางข้อความ — **บั๊กที่มองไม่เห็นในโค้ด**
    ///
    /// เจอของจริงตอนทำ P3-3: ข้อความ `BoardFull` ถูกเขียนเป็นสองบรรทัดด้วย
    /// backslash ต่อบรรทัด แล้ว `cargo fmt` ยุบให้เหลือบรรทัดเดียว **โดยเก็บ
    /// ช่องว่างของการย่อหน้าไว้** ผลคือบน status bar มีช่องโหว่กว้าง 14 ตัวอักษร
    /// กลางประโยค · โค้ดอ่านแล้วดูปกติทุกอย่าง เห็นได้ทางเดียวคือเปิดโปรแกรมแล้วดู
    /// (บทเรียนเดียวกับ glyph ที่ฟอนต์ไม่มีตอน P2-9)
    /// ทุกอาการของ "ช่องว่างที่ไม่มีใครตั้งใจใส่" ที่ตรวจด้วยเครื่องได้
    ///
    /// ★ แยกเป็นฟังก์ชันเพื่อให้ทั้ง `Key` และ `Template` เดินกฎชุดเดียวกันเป๊ะ
    /// — กฎที่เขียนสองที่คือกฎที่วันหนึ่งจะเข้มไม่เท่ากัน
    fn padding_problem(text: &str) -> Option<&'static str> {
        // ★ ขึ้นต้น/ลงท้ายทั้งก้อนด้วยช่องว่าง — ไม่มีเหตุผลที่ถูกต้องเลย
        if text.starts_with(char::is_whitespace) {
            return Some("ขึ้นต้นด้วยช่องว่าง");
        }
        if text.ends_with(char::is_whitespace) {
            return Some("ลงท้ายด้วยช่องว่าง");
        }
        for (index, line) in text.lines().enumerate() {
            // ★★ บรรทัดที่สองเป็นต้นไปขึ้นต้นด้วยช่องว่าง = **ย่อหน้าของโค้ดหลุดเข้ามา**
            //    นี่คืออาการเป๊ะ ๆ ของ `ErrBadHeader`/`ErrDecode` ที่หลุดถึงจอผู้ใช้
            //    มาตั้งแต่ตอนทำ i18n (แม้ช่องว่างตัวเดียวก็ผิด — ไม่ต้องรอให้ครบสองตัว)
            if index > 0 && line.starts_with(char::is_whitespace) {
                return Some("บรรทัดต่อมาขึ้นต้นด้วยช่องว่าง (ย่อหน้าของโค้ดหลุดเข้ามา)");
            }
            if line.contains("  ") {
                return Some("มีช่องว่างติดกันกลางบรรทัด");
            }
            if line.ends_with(char::is_whitespace) {
                return Some("บรรทัดลงท้ายด้วยช่องว่าง");
            }
        }
        None
    }

    #[test]
    fn no_string_is_secretly_padded_with_whitespace() {
        // ★ เก็บให้ครบแล้วค่อยล้ม — ล้มที่ตัวแรกทำให้ต้องรันซ้ำทีละรอบกว่าจะเห็นทั้งหมด
        //   (ของจริงมีสามจุดพร้อมกันตอนประตูนี้ถูกเพิ่มเข้ามา)
        let mut bad = Vec::new();
        for &lang in &[Lang::En, Lang::Th] {
            for &key in ALL_KEYS {
                if let Some(why) = padding_problem(t(lang, key)) {
                    bad.push(format!("{lang:?} {key:?}: {why} — {:?}", t(lang, key)));
                }
            }
            for &tpl in ALL_TEMPLATES {
                let text = template(lang, tpl);
                if let Some(why) = padding_problem(text) {
                    bad.push(format!("{lang:?} {tpl:?}: {why} — {text:?}"));
                }
            }
        }
        assert!(bad.is_empty(), "ข้อความที่มีช่องว่างเกินมา:\n{}", bad.join("\n"));
    }

    /// ★★ negative control ของประตูข้างบน — ใส่ช่องว่างกลับเข้าไปแล้วมันต้องจับได้
    ///
    /// ทุกแบบที่เคยหลุดจริงหรือหลุดได้ ต้องมีตัวอย่างอยู่ที่นี่ · ประตูที่ไม่มีใคร
    /// พิสูจน์ว่าล้มเป็นคือประตูที่อาจเขียวโดยไม่ได้ตรวจอะไร (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn the_padding_gate_catches_every_shape_of_the_bug() {
        // รูปแบบที่เจอจริงใน `ErrBadHeader`/`ErrDecode`: newline ดิบ + ย่อหน้าของโค้ด
        assert!(padding_problem("Damaged file\n             Try again").is_some());
        // รูปแบบที่ `cargo fmt` ผลิตให้ตอนยุบ `\` ต่อบรรทัด: ช่องว่างกลางประโยค
        assert!(padding_problem("6928 of the 10000 you opened     could not be added").is_some());
        // ช่องว่างตัวเดียวหน้าบรรทัดต่อมาก็ผิด — ไม่ต้องรอให้ครบสองตัว
        assert!(padding_problem("line one\n line two").is_some());
        assert!(padding_problem(" leading").is_some());
        assert!(padding_problem("trailing ").is_some());
        assert!(padding_problem("line ends with a space \nnext").is_some());

        // ...และของที่ถูกต้องต้องผ่าน ไม่งั้นประตูนี้จะจับทุกอย่างจนไม่มีความหมาย
        assert!(padding_problem("Ready").is_none());
        assert!(padding_problem("Damaged file\nTry again").is_none());
        assert!(padding_problem("Opened {n} files in {ms} ms").is_none());
    }

    /// ★ สลับภาษาแล้วต้องได้คนละข้อความจริง ไม่ใช่คืนอังกฤษทั้งคู่
    #[test]
    fn switching_language_actually_changes_the_text() {
        // ถ้าคำแปลไทยหายไปทั้งชุด เทสต์นี้จะล้มทันที
        let changed = ALL_KEYS
            .iter()
            .filter(|&&key| t(Lang::En, key) != t(Lang::Th, key))
            .count();
        assert!(
            changed >= ALL_KEYS.len() / 2,
            "แปลไทยแค่ {changed} จาก {} key",
            ALL_KEYS.len()
        );

        assert_eq!(t(Lang::En, Key::Ready), "Ready");
        assert_eq!(t(Lang::Th, Key::Ready), "พร้อมใช้งาน");
    }

    /// ★ คำแปลที่ยังไม่มีต้องตกกลับเป็นอังกฤษ — ห้ามได้ key ห้ามได้ช่องว่าง
    ///
    /// จำลองด้วยการเรียกเส้นทาง fallback ตรง ๆ เพราะตอนนี้แปลครบทุกตัวแล้ว
    #[test]
    fn missing_translation_falls_back_to_english() {
        // `pick` คือกติกาเดียวกับที่ `t` และ `template` ใช้จริง ไม่ใช่โค้ดจำลอง
        assert_eq!(pick(None, en(Key::Ready)), "Ready");
        assert_eq!(pick(Some("พร้อมใช้งาน"), en(Key::Ready)), "พร้อมใช้งาน");
        assert_eq!(
            pick(None, template_en(Template::Loading)),
            "Loading {done} / {total}"
        );

        // ★ สิ่งที่ห้ามเกิดเด็ดขาด: ช่องว่างหรือชื่อ key โผล่ใส่หน้าผู้ใช้
        assert!(!pick(None, en(Key::Ready)).is_empty());
        assert!(!pick(None, en(Key::Ready)).contains("Key::"));
    }

    #[test]
    fn fill_replaces_every_placeholder() {
        let text = fill(
            Lang::En,
            Template::Loading,
            &[("done", "312"), ("total", "1000")],
        );
        assert_eq!(text, "Loading 312 / 1000");
        assert!(!text.contains('{'), "ยังมีตัวยึดตำแหน่งเหลืออยู่: {text}");

        let thai = fill(
            Lang::Th,
            Template::Loading,
            &[("done", "312"), ("total", "1000")],
        );
        assert_eq!(thai, "กำลังโหลด 312 / 1000");
    }

    /// เทมเพลตทุกตัวต้องถูกเติมจนไม่เหลือตัวยึดตำแหน่ง เมื่อผู้เรียกส่งค่าครบ
    ///
    /// จับกรณีที่แปลไทยแล้วพิมพ์ชื่อตัวแปรผิด เช่น `{total}` เป็น `{totol}`
    /// ซึ่งจะโผล่เป็นข้อความประหลาดใส่หน้าผู้ใช้
    #[test]
    fn thai_templates_use_the_same_placeholders_as_english() {
        for &tpl in ALL_TEMPLATES {
            let names = |text: &str| {
                let mut found: Vec<String> = text
                    .split('{')
                    .skip(1)
                    .filter_map(|rest| rest.split('}').next().map(ToOwned::to_owned))
                    .collect();
                found.sort();
                found
            };
            assert_eq!(
                names(template_en(tpl)),
                names(template(Lang::Th, tpl)),
                "{tpl:?} ใช้ตัวยึดตำแหน่งไม่ตรงกันระหว่างสองภาษา"
            );
        }
    }

    // ---------- ข้อความ error ที่ประกอบจากฟิลด์ ----------

    /// ★ ข้อความที่ผู้ใช้เห็นต้องมี **ตัวเลขจริงจากฟิลด์** ไม่ใช่ข้อความลอย ๆ
    /// และต้องบอก "ทำอะไรต่อได้" ตามที่ CLAUDE.md บังคับ
    #[test]
    fn image_too_large_shows_real_numbers_and_a_way_out() {
        let err = LoadError::ImageTooLarge {
            width: 65_535,
            height: 65_535,
            pixels: 4_294_836_225,
            limit: 268_435_456,
        };
        for lang in [Lang::En, Lang::Th] {
            let message = load_error(lang, &err);
            assert!(message.contains("65535"), "{lang:?}: {message}");
            assert!(message.contains("268435456"), "{lang:?}: {message}");
            assert!(!message.contains('{'), "{lang:?}: ยังมีตัวยึดเหลือ {message}");
            // บรรทัดที่สองคือ "ทำอะไรต่อได้"
            assert!(message.lines().count() >= 2, "{lang:?}: {message}");
        }
    }

    /// ★ CLAUDE.md: ข้อความที่ผู้ใช้เห็นต้องบอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ**
    ///
    /// บังคับด้วยเทสต์แทนวินัย — ข้อความ error ทุกตัวต้องมีอย่างน้อยสองบรรทัด
    /// ทั้งสองภาษา ไม่งั้นผู้ใช้รู้แค่ว่า "พัง" แต่ไม่รู้ว่าจะทำอะไรต่อ
    #[test]
    fn every_error_message_tells_the_user_what_to_do_next() {
        let error_templates = ALL_TEMPLATES
            .iter()
            .filter(|tpl| format!("{tpl:?}").starts_with("Err"));
        for &tpl in error_templates {
            for lang in [Lang::En, Lang::Th] {
                let text = template(lang, tpl);
                assert!(
                    text.lines().count() >= 2,
                    "{tpl:?} ({lang:?}) บอกแค่ว่าพัง ไม่ได้บอกว่าทำอะไรต่อได้: {text}"
                );
            }
        }
    }

    #[test]
    fn every_load_error_variant_has_a_user_message() {
        let cases = [
            LoadError::FileTooLarge {
                actual_mb: 600,
                limit_mb: 512,
            },
            LoadError::ImageTooLarge {
                width: 9,
                height: 9,
                pixels: 81,
                limit: 64,
            },
            LoadError::UnknownFormat,
            LoadError::FormatNotAllowed {
                // หยิบจากรายการที่ refx-asset ประกาศไว้ เพื่อไม่ต้องดึง `image`
                // เข้ามาเป็น dependency ของ crate นี้เพียงเพื่อเขียนเทสต์
                format: refx_asset::decode::ALLOWED_FORMATS[0],
            },
            LoadError::BadHeader,
            LoadError::DecoderPanic,
            LoadError::Io {
                file: "cat.png".to_owned(),
                source: std::io::Error::other("x"),
            },
            LoadError::NotAFile {
                file: "folder".to_owned(),
            },
        ];
        for err in &cases {
            for lang in [Lang::En, Lang::Th] {
                let message = load_error(lang, err);
                assert!(!message.trim().is_empty(), "{err:?} ({lang:?}) ว่างเปล่า");
                assert!(!message.contains('{'), "{err:?} ({lang:?}): {message}");
            }
        }
    }

    /// ★ ชื่อไฟล์ต้องไปถึงผู้ใช้ — "เปิดไฟล์ไหนไม่ได้" คือสิ่งแรกที่เขาอยากรู้
    #[test]
    fn io_error_names_the_file() {
        let err = LoadError::Io {
            file: "ภาพอ้างอิง.png".to_owned(),
            source: std::io::Error::other("boom"),
        };
        for lang in [Lang::En, Lang::Th] {
            assert!(load_error(lang, &err).contains("ภาพอ้างอิง.png"));
        }
    }

    #[test]
    fn timeout_message_names_the_file_and_the_limit() {
        let err = JobFailure::Timeout {
            file: "big.tif".to_owned(),
            seconds: 20,
        };
        for lang in [Lang::En, Lang::Th] {
            let message = job_failure(lang, &err);
            assert!(message.contains("big.tif"), "{message}");
            assert!(message.contains("20"), "{message}");
        }
    }

    #[test]
    fn atlas_errors_report_their_numbers() {
        let full = AtlasError::Full { layers: 12 };
        assert!(atlas_error(Lang::En, &full).contains("12"));

        let vram = AtlasError::OutOfVram(refx_render::texture::VramError::OverBudget {
            requested_mb: 192,
            used_mb: 176,
            limit_mb: 384,
        });
        for lang in [Lang::En, Lang::Th] {
            let message = atlas_error(lang, &vram);
            assert!(
                message.contains("192") && message.contains("384"),
                "{message}"
            );
        }
    }

    /// ★ วางแล้วไม่ได้ภาพ ต้องรู้ว่า **ทำไม** และ **ทำอะไรต่อ** ไม่ใช่เงียบไป
    ///
    /// เคสที่เจอบ่อยที่สุดคือก๊อปข้อความมาแล้วกด Ctrl+V — ถ้าไม่มีข้อความบอก
    /// ผู้ใช้จะนึกว่าโปรแกรมพัง ทั้งที่ทำงานถูกต้องทุกอย่าง
    #[test]
    fn every_clipboard_error_variant_has_a_user_message() {
        let cases = [
            ClipboardError::NoImage,
            ClipboardError::Busy,
            ClipboardError::Unavailable {
                reason: "no display".to_owned(),
            },
            ClipboardError::Undecodable,
        ];
        let mut seen: Vec<String> = Vec::new();
        for err in &cases {
            for lang in [Lang::En, Lang::Th] {
                let message = clipboard_error(lang, err);
                assert!(!message.trim().is_empty(), "{err:?} ({lang:?}) ว่างเปล่า");
                assert!(!message.contains('{'), "{err:?} ({lang:?}): {message}");
                // บรรทัดที่สองคือ "ทำอะไรต่อได้"
                assert!(
                    message.lines().count() >= 2,
                    "{err:?} ({lang:?}): {message}"
                );
            }
            // แต่ละสาเหตุต้องได้ข้อความคนละแบบ ไม่ใช่ "วางไม่ได้" เหมือนกันหมด
            seen.push(clipboard_error(Lang::En, err));
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), cases.len(), "มีสาเหตุที่ใช้ข้อความซ้ำกัน");
    }

    /// error ของ clipboard ต้องเดินผ่าน `job_failure` ได้เหมือนสาเหตุอื่น
    #[test]
    fn clipboard_failure_reaches_the_user_through_job_failure() {
        let err = JobFailure::Clipboard(ClipboardError::NoImage);
        for lang in [Lang::En, Lang::Th] {
            assert_eq!(
                job_failure(lang, &err),
                clipboard_error(lang, &ClipboardError::NoImage)
            );
        }
    }

    /// ภาพดิบที่ข้อมูลไม่ครบต้องบอกขนาดจริงที่ประกาศมา
    #[test]
    fn malformed_pixels_message_shows_the_declared_size() {
        let err = LoadError::MalformedPixels {
            width: 1920,
            height: 1080,
            expected: 8_294_400,
            actual: 12,
        };
        for lang in [Lang::En, Lang::Th] {
            let message = load_error(lang, &err);
            assert!(
                message.contains("1920") && message.contains("1080"),
                "{message}"
            );
            assert!(!message.contains('{'), "{message}");
        }
    }

    // ---------- การเลือกภาษาจาก OS ----------

    #[test]
    fn language_is_picked_from_the_os_tag() {
        assert_eq!(Lang::from_tag("th-TH"), Lang::Th);
        assert_eq!(Lang::from_tag("th_TH.UTF-8"), Lang::Th);
        assert_eq!(Lang::from_tag("en-US"), Lang::En);
        // ภาษาที่ยังไม่รองรับต้องได้อังกฤษ ไม่ใช่ค้างหรือ panic
        assert_eq!(Lang::from_tag("ja-JP"), Lang::En);
        assert_eq!(Lang::from_tag("zh-Hans-CN"), Lang::En);
        assert_eq!(Lang::from_tag(""), Lang::En);
        assert_eq!(Lang::from_tag("!!!"), Lang::En);
        assert_eq!(Lang::default(), Lang::En);
    }
}
