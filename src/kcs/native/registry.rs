#[derive(Clone, Copy)]
pub(super) enum CoreHandler {
    Text,
    Scalar,
    Settings,
    Storage,
    System,
    Data,
    MapData,
    Sequence,
    RecordStack,
    Vector,
    Continue,
    Zero,
    Unknown,
}

pub(super) fn canonical(index: u32) -> u32 {
    match index {
        675 | 885 | 894 => 515,
        676 | 886 | 895 => 516,
        888 => 897,
        889 => 898,
        890 => 899,
        893 => 902,
        _ => index,
    }
}

pub(super) fn core(index: u32) -> CoreHandler {
    match index {
        13..=26
        | 37
        | 38
        | 40..=48
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
        33 | 34 | 50..=64 | 218 | 318..=320 | 397 | 461 | 874 | 875 => CoreHandler::Scalar,
        359..=361
        | 533
        | 565
        | 566
        | 605
        | 606
        | 607
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
        | 1062 => CoreHandler::Settings,
        243
        | 253
        | 419..=423
        | 459
        | 515..=518
        | 869
        | 870
        | 872..=873
        | 896..=899
        | 902
        | 1052 => CoreHandler::Storage,
        238 | 242 | 244..=248 | 252 | 254 | 290 | 389 | 390 | 469 | 833..=836 => CoreHandler::Data,
        900 | 901 => CoreHandler::MapData,
        424 | 425 => CoreHandler::Sequence,
        863..=868 => CoreHandler::RecordStack,
        350..=358 => CoreHandler::Vector,
        1
        | 4
        | 5
        | 6
        | 8
        | 9
        | 35
        | 36
        | 65
        | 68
        | 71
        | 75
        | 76
        | 149
        | 207
        | 209..=213
        | 787
        | 392
        | 393
        | 465
        | 629
        | 1040
        | 1049 => CoreHandler::System,
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
        _ => CoreHandler::Unknown,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RuntimeHandler {
    Property,
    Configuration,
    Emocon,
    Backlog,
    LoadProgram,
    LoadParticle,
    Movie,
    Transition,
    NameTable,
    Cst,
    Motion,
    SequenceFile,
    SharedRecord,
    SoundLoop,
    SoundControl,
    SoundLoad,
    Input,
    Message,
    Fes,
    PointerPosition,
    Graphics,
    Task,
    MediaRecord,
    Resource,
    ImageResource,
    Core,
}

pub(crate) fn runtime(index: u32) -> RuntimeHandler {
    match index {
        453 => RuntimeHandler::Property,
        361 => RuntimeHandler::Configuration,
        1055..=1061 => RuntimeHandler::Emocon,
        696..=700 | 785 | 850 | 851 | 876 | 977 | 1034 => RuntimeHandler::Backlog,
        143 => RuntimeHandler::LoadProgram,
        271 | 693 => RuntimeHandler::LoadParticle,
        265..=270 | 417 | 788 | 997 | 998 => RuntimeHandler::Movie,
        196..=200 | 203 | 520 | 527 | 528 | 530..=532 | 853..=857 | 860 | 861 | 905 | 967..=969 => {
            RuntimeHandler::Transition
        }
        633..=638 => RuntimeHandler::NameTable,
        323..=337 | 385 | 386 | 934 | 982 | 991 | 992 | 994 | 1033 | 1037 | 1038 | 1064 => {
            RuntimeHandler::Cst
        }
        294..=317 | 388 | 440 | 535 | 586 => RuntimeHandler::Motion,
        458 => RuntimeHandler::SequenceFile,
        240 | 250 => RuntimeHandler::SharedRecord,
        400 => RuntimeHandler::SoundLoop,
        156..=168 | 347..=349 | 903 => RuntimeHandler::SoundControl,
        935 | 936 => RuntimeHandler::SoundLoad,
        145..=148 | 154 | 285 | 286 | 401 | 403 | 404 | 937 => RuntimeHandler::Input,
        611..=621 | 695 | 884 => RuntimeHandler::Message,
        447..=449 | 451 | 630 | 645..=647 | 649 | 650 | 882 | 883 => RuntimeHandler::Fes,
        149 => RuntimeHandler::PointerPosition,
        113
        | 78
        | 79
        | 80
        | 82
        | 83
        | 84..=89
        | 91..=107
        | 256
        | 258
        | 260
        | 261
        | 262
        | 263
        | 339..=342
        | 343..=345
        | 398
        | 399
        | 402
        | 455
        | 732
        | 462..=464
        | 466
        | 534
        | 542
        | 568
        | 570
        | 571
        | 572
        | 625
        | 653
        | 657
        | 663
        | 739
        | 848
        | 849
        | 858
        | 859
        | 862
        | 879
        | 880
        | 881
        | 1065..=1069 => RuntimeHandler::Graphics,
        66 | 67 | 69 | 70 | 72 | 73 | 75 | 76 | 108..=110 | 272..=275 | 426 | 427 | 452 | 694 => {
            RuntimeHandler::Task
        }
        241 | 251 | 387 => RuntimeHandler::MediaRecord,
        111 | 112 | 114..=117 | 284 | 362 | 365 | 366 | 368 | 434..=438 | 536 | 590..=597 => {
            RuntimeHandler::Resource
        }
        710..=715 | 717..=724 | 726..=728 | 730 | 733 | 735..=737 => RuntimeHandler::ImageResource,
        _ => RuntimeHandler::Core,
    }
}
