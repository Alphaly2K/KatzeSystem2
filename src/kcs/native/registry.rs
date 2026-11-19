#[derive(Clone, Copy)]
pub(super) enum CoreHandler {
    Text,
    Memory,
    Rectangle,
    Scalar,
    Settings,
    Storage,
    File,
    Database,
    System,
    Window,
    Achievements,
    ScriptSettings,
    Matrix,
    Data,
    NamePairs,
    MapData,
    Sequence,
    RecordQueue,
    RecordStack,
    Vector,
    Continue,
    Zero,
    UninitializedResult,
    Unknown,
}

pub(crate) fn canonical(index: u32) -> u32 {
    match index {
        705 => 122,
        675 | 885 | 894 => 515,
        676 | 886 | 895 => 516,
        887 => 896,
        888 => 897,
        889 => 898,
        890 => 899,
        891 => 900,
        892 => 901,
        893 => 902,
        _ => index,
    }
}

pub(super) fn core(index: u32) -> CoreHandler {
    match index {
        1010..=1013 => CoreHandler::RecordQueue,
        441 | 457 | 628 | 845..=847 | 1054 => CoreHandler::Window,
        1041..=1044 => CoreHandler::Achievements,
        192..=195 => CoreHandler::ScriptSettings,
        29..=32 => CoreHandler::File,
        27 | 28 | 35 | 36 | 237 => CoreHandler::Memory,
        214 | 215 | 217 | 218 => CoreHandler::Rectangle,
        13..=26
        | 37..=49
        | 276..=278
        | 519
        | 539
        | 540
        | 598
        | 1002
        | 1045
        | 1046
        | 1051
        | 1063 => CoreHandler::Text,
        904 => CoreHandler::Text,
        33 | 34 | 50..=64 | 318..=321 | 397 | 461 | 786 | 874 | 875 => CoreHandler::Scalar,
        359..=361
        | 533
        | 565
        | 566
        | 582..=584
        | 589
        | 605
        | 606
        | 607
        | 674
        | 741
        | 789
        | 790
        | 844
        | 852
        | 938..=964
        | 995
        | 999..=1001
        | 1036
        | 1050
        | 1053
        | 1062
        | 1077..=1079 => CoreHandler::Settings,
        243 | 253 | 419..=423 | 459 | 515..=518 | 677..=680 | 896..=899 | 902 | 1052 => {
            CoreHandler::Storage
        }
        869..=873 | 878 => CoreHandler::Database,
        238
        | 242
        | 244..=248
        | 252
        | 254..=255
        | 290
        | 389
        | 390
        | 469
        | 833..=836
        | 990
        | 1031 => CoreHandler::Data,
        900 | 901 => CoreHandler::MapData,
        289 | 291..=293 => CoreHandler::NamePairs,
        424 | 425 => CoreHandler::Sequence,
        863..=868 => CoreHandler::RecordStack,
        350..=358 => CoreHandler::Vector,
        204..=207 | 662 => CoreHandler::Matrix,
        1
        | 4
        | 5
        | 6
        | 8
        | 9
        | 10
        | 11
        | 65
        | 68
        | 71
        | 75
        | 76
        | 149
        | 209..=213
        | 567
        | 787
        | 392
        | 393
        | 442..=445
        | 465
        | 609
        | 610
        | 629
        | 658
        | 659
        | 1024
        | 1039
        | 1040
        | 1049
        | 1074 => CoreHandler::System,
        0
        | 2
        | 3
        | 141
        | 169
        | 170
        | 172
        | 173
        | 180
        | 182
        | 184
        | 185
        | 187..=191
        | 429
        | 439
        | 543..=548
        | 550
        | 552
        | 553
        | 556
        | 559..=563
        | 573
        | 574
        | 578
        | 579
        | 581
        | 626
        | 627
        | 642..=644
        | 652
        | 656
        | 660
        | 661
        | 670..=673
        | 686
        | 687
        | 906..=908
        | 910..=924
        | 926..=933
        | 965
        | 966
        | 974..=976
        | 1006
        | 1025
        | 1047
        | 1048 => CoreHandler::Continue,
        171
        | 174..=179
        | 181
        | 183
        | 186
        | 236
        | 432
        | 551
        | 554
        | 555
        | 575
        | 580
        | 909
        | 925
        | 1027..=1030 => CoreHandler::Zero,
        140 | 142 | 549 => CoreHandler::UninitializedResult,
        _ => CoreHandler::Unknown,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RuntimeHandler {
    FileOpen,
    Property,
    Configuration,
    DebugWindow,
    Export,
    SystemTask,
    PlatformJob,
    Effect,
    ImageExport,
    Emocon,
    Backlog,
    LoadProgram,
    LoadParticle,
    Movie,
    Transition,
    NameTable,
    SectionTable,
    DatabaseTable,
    Quiz,
    QuizOptions,
    Cst,
    Motion,
    SequenceFile,
    DatabaseLoad,
    SharedRecord,
    SharedNumbers,
    SoundLoop,
    SoundControl,
    SoundLoad,
    Input,
    Message,
    TextWindow,
    Fes,
    PointerPosition,
    Graphics,
    Task,
    MediaRecord,
    StateRecord,
    Resource,
    ImageAnimation,
    ImageResource,
    ImagePack,
    Core,
}

pub(crate) fn runtime(index: u32) -> RuntimeHandler {
    match canonical(index) {
        1009 => RuntimeHandler::ImageExport,
        430 | 624 => RuntimeHandler::ImageAnimation,
        703 | 704 | 706 | 709 => RuntimeHandler::ImagePack,
        208 | 600 | 601 | 1018..=1023 => RuntimeHandler::SystemTask,
        1003..=1005 | 1007 | 1008 => RuntimeHandler::PlatformJob,
        1014..=1017 | 1076 => RuntimeHandler::Effect,
        123..=139 | 391 | 418 | 456 | 557 | 585 | 599 => RuntimeHandler::TextWindow,
        29 => RuntimeHandler::FileOpen,
        453 => RuntimeHandler::Property,
        361 => RuntimeHandler::Configuration,
        279..=283 | 287 | 288 | 837..=843 => RuntimeHandler::DebugWindow,
        690..=692 => RuntimeHandler::Export,
        1055..=1061 => RuntimeHandler::Emocon,
        696..=700 | 785 | 850 | 851 | 876 | 977 | 1034 => RuntimeHandler::Backlog,
        143 => RuntimeHandler::LoadProgram,
        271 | 669 | 693 => RuntimeHandler::LoadParticle,
        265..=270 | 417 | 788 | 996..=998 => RuntimeHandler::Movie,
        196..=203 | 520..=532 | 853..=857 | 860 | 861 | 905 | 967..=973 => {
            RuntimeHandler::Transition
        }
        633..=640 => RuntimeHandler::NameTable,
        683..=685 => RuntimeHandler::SectionTable,
        791..=794 => RuntimeHandler::DatabaseTable,
        742..=778 => RuntimeHandler::Quiz,
        779..=784 | 829..=832 => RuntimeHandler::QuizOptions,
        219..=235
        | 239
        | 249
        | 323..=338
        | 608
        | 379..=386
        | 934
        | 978
        | 979
        | 980
        | 981
        | 982
        | 991..=994
        | 1032
        | 1033
        | 1035
        | 1037
        | 1038
        | 1064 => RuntimeHandler::Cst,
        294..=317 | 388 | 440 | 514 | 535 | 586 => RuntimeHandler::Motion,
        458 => RuntimeHandler::SequenceFile,
        869 => RuntimeHandler::DatabaseLoad,
        240 | 250 => RuntimeHandler::SharedRecord,
        609 | 610 | 658 | 659 => RuntimeHandler::SharedNumbers,
        400 => RuntimeHandler::SoundLoop,
        156..=168 | 347..=349 | 569 | 587 | 588 | 602 | 603 | 903 => RuntimeHandler::SoundControl,
        155 | 604 | 935 | 936 | 1070..=1073 | 1075 => RuntimeHandler::SoundLoad,
        144..=148 | 150..=154 | 285 | 286 | 395 | 396 | 401 | 403 | 404 | 937 => {
            RuntimeHandler::Input
        }
        611..=621 | 695 | 884 => RuntimeHandler::Message,
        447..=451 | 630..=632 | 645..=647 | 649 | 650 | 664..=668 | 738 | 882 | 883 => {
            RuntimeHandler::Fes
        }
        149 => RuntimeHandler::PointerPosition,
        113
        | 78
        | 79
        | 80
        | 81
        | 82
        | 83
        | 84..=107
        | 256..=264
        | 339..=342
        | 343..=345
        | 346
        | 394
        | 398
        | 399
        | 402
        | 433
        | 455
        | 731..=732
        | 462..=464
        | 466
        | 467
        | 534
        | 541
        | 542
        | 568
        | 570
        | 571
        | 572
        | 622
        | 623
        | 625
        | 653
        | 654
        | 655
        | 657
        | 663
        | 701
        | 702
        | 707..=708
        | 739
        | 740
        | 848
        | 849
        | 858
        | 859
        | 862
        | 879
        | 880
        | 881
        | 988
        | 989
        | 1065..=1069 => RuntimeHandler::Graphics,
        66
        | 67
        | 69
        | 70
        | 72
        | 73
        | 74
        | 75
        | 76
        | 77
        | 108..=110
        | 272..=275
        | 426
        | 427
        | 452
        | 454
        | 577
        | 651
        | 694 => RuntimeHandler::Task,
        241 | 251 | 387 => RuntimeHandler::MediaRecord,
        576 | 688 | 689 => RuntimeHandler::StateRecord,
        111
        | 112
        | 216
        | 114..=120
        | 121
        | 122
        | 284
        | 322
        | 362..=378
        | 405..=416
        | 428
        | 431
        | 434..=438
        | 446
        | 468
        | 536..=538
        | 558
        | 564
        | 590..=597
        | 641
        | 1026 => RuntimeHandler::Resource,
        460 | 681 | 682 | 710..=730 | 733..=737 | 877 => RuntimeHandler::ImageResource,
        _ => RuntimeHandler::Core,
    }
}
